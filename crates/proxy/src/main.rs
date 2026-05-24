// SPDX-License-Identifier: Apache-2.0

//! Spec §5.2 — HTTP forwarder between a Nitro Enclave and the public network.
//!
//! ## Listeners
//! - `vendor-aws` (default, Linux only): vsock CID_ANY on `PROXY_LISTEN_PORT`
//!   (default 8000). The only legitimate inbound peer is an enclave
//!   (CID ≥ 16); accept-time ACL rejects loopback (CID 1) and host (CID 2)
//!   to limit blast radius if a sibling host process opens a vsock socket.
//! - otherwise: TCP `127.0.0.1:8000` (dev mode).

mod handler;
mod protocol;

const DEFAULT_LISTEN_PORT: u32 = 8000;

fn listen_port() -> u32 {
    std::env::var("PROXY_LISTEN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_LISTEN_PORT)
}

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let port = listen_port();
    tracing::info!(port, "title-proxy starting on vsock");

    let listener = vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, port)?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<vsock::VsockStream>(32);

    // Minimum CID accepted from peers. AWS Nitro assigns enclave CIDs
    // starting at 16; values 0–2 are reserved (hypervisor / local loopback /
    // host). Rejecting < 3 means a co-tenant host process cannot connect
    // to this proxy via vsock loopback.
    const MIN_ACCEPTED_CID: u32 = 3;

    std::thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok((s, peer)) => {
                    let peer_cid = peer.cid();
                    if peer_cid < MIN_ACCEPTED_CID {
                        tracing::warn!(
                            peer_cid,
                            "rejecting vsock connection from reserved CID"
                        );
                        continue;
                    }
                    match tx.try_send(s) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            tracing::warn!(
                                queued = 32,
                                peer_cid,
                                "vsock accept backpressure; dropping incoming connection"
                            );
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            tracing::info!("channel closed; vsock accept loop exiting");
                            break;
                        }
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
    let addr = format!("127.0.0.1:{}", listen_port());
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
// ----------------------------------------------------------------------------

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
async fn handle_vsock_connection(stream: vsock::VsockStream) {
    use crate::protocol::{MAX_METHOD_BYTES, MAX_REQUEST_BODY_BYTES, MAX_URL_BYTES};

    let read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "vsock try_clone failed; closing connection");
            return;
        }
    };

    let read_result = tokio::task::spawn_blocking(move || {
        let mut s = read_stream;
        let method = protocol::read_string_sync(&mut s, MAX_METHOD_BYTES)?;
        let url = protocol::read_string_sync(&mut s, MAX_URL_BYTES)?;
        let body = protocol::read_bytes_sync(&mut s, MAX_REQUEST_BODY_BYTES)?;
        Ok::<_, std::io::Error>((method, url, body))
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

    /// Tokio `AsyncWrite` shim over the blocking `vsock::VsockStream`. Each
    /// `poll_write` blocks the worker thread for the duration of a single
    /// `write(2)` — fine here because connections are one-shot and short.
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
            use std::net::Shutdown;
            // Half-close the write side so the upstream sees a clean EOF.
            // A failure here is best-effort: the OS will tear the socket
            // down when the wrapper drops anyway.
            let _ = self.get_mut().0.shutdown(Shutdown::Write);
            Poll::Ready(Ok(()))
        }
    }

    // Safety: `vsock::VsockStream` owns a single OS file descriptor (no
    // interior mutability, no thread-affine handles). The wrapper is only
    // ever created from `handle_vsock_connection` which immediately moves
    // it into a `tokio::spawn`ed task and uses it from that single task
    // — there is no concurrent access from another thread. We need to
    // assert `Send` explicitly because `vsock 0.5` does not.
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
        use crate::protocol::MAX_RESPONSE_BYTES;
        let status = protocol::read_u32_async(r).await.unwrap();
        let body = protocol::read_bytes_async(r, MAX_RESPONSE_BYTES as usize).await.unwrap();
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

    #[tokio::test]
    async fn chunked_get_uses_sentinel() {
        use axum::body::Body;
        use axum::http::Response;
        use crate::protocol::CHUNKED_SENTINEL;

        // Upstream that sends Transfer-Encoding: chunked with no Content-Length.
        let app = Router::new().route(
            "/stream",
            get(|| async {
                let s = futures_util::stream::iter(vec![
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"hello ")),
                    Ok(axum::body::Bytes::from_static(b"world")),
                ]);
                Response::builder().body(Body::from_stream(s)).unwrap()
            }),
        );
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(upstream_listener, app).await.unwrap();
        });

        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        write_request(
            &mut stream,
            "GET",
            &format!("http://127.0.0.1:{upstream_port}/stream"),
            &[],
        )
        .await;

        use tokio::io::AsyncReadExt;
        let status = protocol::read_u32_async(&mut stream).await.unwrap();
        assert_eq!(status, 200);
        let body_len = protocol::read_u32_async(&mut stream).await.unwrap();
        assert_eq!(body_len, CHUNKED_SENTINEL);

        let mut collected = Vec::new();
        loop {
            let n = protocol::read_u32_async(&mut stream).await.unwrap() as usize;
            if n == 0 {
                break;
            }
            let mut buf = vec![0u8; n];
            stream.read_exact(&mut buf).await.unwrap();
            collected.extend(buf);
        }
        assert_eq!(collected, b"hello world");
    }
}
