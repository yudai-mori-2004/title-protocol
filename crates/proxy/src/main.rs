// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol HTTP forwarder
//!
//! Spec §5.2 — TEE content fetch (proxy-mediated transport)
//!
//! Runs on the EC2 host alongside the Nitro Enclave. The Enclave has no
//! network interface, so every outbound HTTP/HTTPS call from the TEE is
//! tunneled here over vsock. We then re-issue it on the public network
//! and stream the response back.
//!
//! ## Listeners
//! - `vendor-aws` (default, Linux only): vsock CID_ANY port 8000
//! - otherwise: TCP 127.0.0.1:8000 (dev mode)

mod handler;
mod protocol;

const LISTEN_PORT: u32 = 8000;

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!(port = LISTEN_PORT, "title-proxy starting on vsock");

    let listener = vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, LISTEN_PORT)?;

    // vsock accept is blocking; run it on a dedicated thread and ship
    // accepted connections to the tokio runtime via a bounded channel.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<vsock::VsockStream>(32);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    if tx.blocking_send(s).is_err() {
                        tracing::info!("channel closed; vsock accept loop exiting");
                        break;
                    }
                }
                Err(e) => tracing::error!(error = %e, "vsock accept error"),
            }
        }
    });

    while let Some(stream) = rx.recv().await {
        tokio::spawn(handle_vsock_connection(stream));
    }

    Ok(())
}

#[cfg(not(all(target_os = "linux", feature = "vendor-aws")))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let addr = format!("127.0.0.1:{LISTEN_PORT}");
    tracing::info!(addr = %addr, "title-proxy starting on TCP (dev mode)");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    loop {
        let (stream, peer) = listener.accept().await?;
        tracing::debug!(peer = %peer, "accepted TCP connection");
        tokio::spawn(handler::handle_tcp_connection(stream));
    }
}

// ----------------------------------------------------------------------------
// vsock connection handler (Linux + vendor-aws only).
//
// The `vsock` crate exposes blocking `Read`/`Write`. We read the request on
// a blocking task, then wrap the write half in a tokio AsyncWrite shim so
// the streaming forwarder in `handler.rs` works without rewrites.
// ----------------------------------------------------------------------------

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
async fn handle_vsock_connection(stream: vsock::VsockStream) {
    let read_result = tokio::task::spawn_blocking({
        let mut s = stream.try_clone().expect("vsock try_clone");
        move || {
            let method = protocol::read_string_sync(&mut s)?;
            let url = protocol::read_string_sync(&mut s)?;
            let body = protocol::read_bytes_sync(&mut s)?;
            Ok::<_, std::io::Error>((method, url, body))
        }
    })
    .await;

    let (method, url, body) = match read_result {
        Ok(Ok(parts)) => parts,
        Ok(Err(e)) => {
            tracing::error!(error = %e, "failed to read request from vsock");
            return;
        }
        Err(e) => {
            tracing::error!(error = %e, "vsock read task panicked");
            return;
        }
    };

    tracing::info!(method, url, body_len = body.len(), "vsock request");

    let mut writer = tokio::io::BufWriter::new(vsock_async::VsockWriter(stream));
    if let Err(e) = handler::forward_http_streaming(&mut writer, &method, &url, &body).await {
        tracing::error!(error = %e, "vsock write failed");
    }
}

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
mod vsock_async {
    use std::io::Write;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::AsyncWrite;

    /// Adapts a blocking `vsock::VsockStream` to tokio's `AsyncWrite`.
    /// Writes block the tokio worker thread for the duration of each
    /// `write` call. That's acceptable here because the proxy spawns
    /// one task per connection and connections are short-lived.
    pub struct VsockWriter(pub vsock::VsockStream);

    impl AsyncWrite for VsockWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(self.get_mut().0.write(buf))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(self.get_mut().0.flush())
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    // VsockStream contains a raw fd; sharing it between threads requires
    // the unsafe Send marker. Write access is single-task in this proxy.
    unsafe impl Send for VsockWriter {}
}

// ----------------------------------------------------------------------------
// Tests (TCP path — exercised on every platform).
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Bytes, routing::get, routing::post, Router};
    use tokio::io::AsyncWriteExt;

    async fn start_mock_upstream() -> u16 {
        let app = Router::new()
            .route("/hello", get(|| async { "hi" }))
            .route("/echo", post(|body: Bytes| async move { body }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    }

    async fn start_proxy() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(handler::handle_tcp_connection(stream));
            }
        });
        port
    }

    async fn write_request<W: tokio::io::AsyncWrite + Unpin>(
        w: &mut W,
        method: &str,
        url: &str,
        body: &[u8],
    ) {
        w.write_all(&(method.len() as u32).to_be_bytes()).await.unwrap();
        w.write_all(method.as_bytes()).await.unwrap();
        w.write_all(&(url.len() as u32).to_be_bytes()).await.unwrap();
        w.write_all(url.as_bytes()).await.unwrap();
        w.write_all(&(body.len() as u32).to_be_bytes()).await.unwrap();
        w.write_all(body).await.unwrap();
        w.flush().await.unwrap();
    }

    async fn read_response<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> (u32, Vec<u8>) {
        let status = protocol::read_u32_async(r).await.unwrap();
        let body = protocol::read_bytes_async(r).await.unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn get_roundtrip() {
        let upstream = start_mock_upstream().await;
        let proxy = start_proxy().await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        write_request(
            &mut stream,
            "GET",
            &format!("http://127.0.0.1:{upstream}/hello"),
            &[],
        )
        .await;

        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 200);
        assert_eq!(body, b"hi");
    }

    #[tokio::test]
    async fn post_roundtrip() {
        let upstream = start_mock_upstream().await;
        let proxy = start_proxy().await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        let payload = b"{\"key\":\"value\"}";
        write_request(
            &mut stream,
            "POST",
            &format!("http://127.0.0.1:{upstream}/echo"),
            payload,
        )
        .await;

        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 200);
        assert_eq!(body, payload);
    }

    #[tokio::test]
    async fn unsupported_method_rejected() {
        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        write_request(&mut stream, "DELETE", "http://example.com", &[]).await;
        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 400);
        assert!(String::from_utf8(body).unwrap().contains("Unsupported method"));
    }

    #[tokio::test]
    async fn upstream_unreachable_yields_proxy_error() {
        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        // Port 1 is reserved (tcpmux), should reliably refuse.
        write_request(&mut stream, "GET", "http://127.0.0.1:1/nope", &[]).await;
        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 0, "proxy error status");
        assert!(String::from_utf8(body).unwrap().contains("Proxy error"));
    }
}
