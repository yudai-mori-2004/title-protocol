// SPDX-License-Identifier: Apache-2.0

//! Spec §5.2 — HTTP forwarder between a Nitro Enclave and the public network.
//!
//! ## Listeners
//! - `vendor-aws` (default, Linux only): vsock CID_ANY on `PROXY_LISTEN_PORT`
//!   (default 8000). The only legitimate inbound peer is an enclave
//!   (CID ≥ 16); accept-time ACL rejects loopback (CID 1) and host (CID 2)
//!   to limit blast radius if a sibling host process opens a vsock socket.
//! - otherwise: TCP `127.0.0.1:8000` (dev mode).

// `handler` と `protocol` は lib.rs から公開済み。binary では crate ルート
// (= `title_proxy::*`) 経由で参照する。`protocol` は vsock 経路 (Linux + vendor-aws)
// でのみ使うため、cfg 経由で参照されない target では unused 警告を抑制する。
use title_proxy::handler;
#[allow(unused_imports)]
use title_proxy::protocol;

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
    // accept 直後の vsock stream に I/O timeout を設定する。これを入れないと
    // 「接続を開いて 1 バイト送って黙る」攻撃 (slow-read DoS) で tokio の
    // blocking thread pool (デフォルト 512) を埋め尽くされる経路が残る。
    // TEE 側 (`proxy_fetcher.rs::PROXY_IO_TIMEOUT`) と揃えて 60s。
    const ACCEPT_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    std::thread::spawn(move || loop {
        match listener.accept() {
            Ok((s, peer)) => {
                let peer_cid = peer.cid();
                if peer_cid < MIN_ACCEPTED_CID {
                    tracing::warn!(peer_cid, "rejecting vsock connection from reserved CID");
                    continue;
                }
                if let Err(e) = s.set_read_timeout(Some(ACCEPT_IO_TIMEOUT)) {
                    tracing::warn!(error = %e, peer_cid, "failed to set vsock read timeout; dropping connection");
                    continue;
                }
                if let Err(e) = s.set_write_timeout(Some(ACCEPT_IO_TIMEOUT)) {
                    tracing::warn!(error = %e, peer_cid, "failed to set vsock write timeout; dropping connection");
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

    /// Tokio `AsyncWrite` shim over the blocking `vsock::VsockStream`.
    ///
    /// 各 `poll_write` は単一の `write(2)` 呼び出しが完了するまでワーカー
    /// スレッドをブロックする。`MAX_RESPONSE_BYTES = 100 MiB` の chunked GET
    /// が走った場合、外側の `BufWriter`(cap 8 KiB) を経由して同一ワーカで
    /// 最大 12,500 回の syscall がシリアル実行される。これ自体は許容範囲
    /// (各 syscall は accept 時に設定した 60 秒 timeout で必ず戻る) だが、
    /// 「one-shot で short」と言い切れる前提ではない。同時 chunked 接続が
    /// 多い場合は tokio multi-thread runtime のワーカー数 (デフォルト
    /// `available_parallelism()`) で全体スループットが律速される。
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

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
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
    //
    // `VsockWriter` がラップする stream には accept 時に
    // `set_read_timeout` / `set_write_timeout` が掛かっているため、`poll_write`
    // 内の `write(2)` は無期限ブロックしない。
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
        w.write_all(&(method.len() as u32).to_be_bytes())
            .await
            .unwrap();
        w.write_all(method.as_bytes()).await.unwrap();
        w.write_all(&(url.len() as u32).to_be_bytes())
            .await
            .unwrap();
        w.write_all(url.as_bytes()).await.unwrap();
        w.write_all(&(body.len() as u32).to_be_bytes())
            .await
            .unwrap();
        w.write_all(body).await.unwrap();
        w.flush().await.unwrap();
    }

    async fn read_response<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> (u32, Vec<u8>) {
        use crate::protocol::MAX_RESPONSE_BYTES;
        let status = protocol::read_u32_async(r).await.unwrap();
        let body = protocol::read_bytes_async(r, MAX_RESPONSE_BYTES as usize)
            .await
            .unwrap();
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
        // proxy 内部での拒否なので PROXY_ERROR_STATUS (= 0)。上流の HTTP 400
        // と区別できるようにすることで TEE 側で原因の切り分けが可能になる。
        assert_eq!(status, 0, "proxy-internal reject uses status 0");
        assert!(String::from_utf8(body)
            .unwrap()
            .contains("Unsupported method"));
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
    async fn head_returns_structured_response() {
        use crate::protocol::decode_head_response;
        use axum::http::Response;

        // Upstream that responds to HEAD with Content-Length / Accept-Ranges / ETag.
        let app = Router::new().route(
            "/file.mp4",
            axum::routing::head(|| async {
                Response::builder()
                    .status(200)
                    .header("Content-Length", "1048576")
                    .header("Accept-Ranges", "bytes")
                    .header("ETag", "\"abc-123\"")
                    .header("Content-Type", "video/mp4")
                    .body(axum::body::Body::empty())
                    .unwrap()
            }),
        );
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(upstream, app).await.unwrap();
        });

        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        write_request(
            &mut stream,
            "HEAD",
            &format!("http://127.0.0.1:{upstream_port}/file.mp4"),
            &[],
        )
        .await;

        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 200);
        let parsed = decode_head_response(&body).expect("decode HEAD response");
        assert_eq!(parsed.content_length, 1_048_576);
        assert!(parsed.accepts_ranges);
        assert_eq!(parsed.etag.as_deref(), Some("\"abc-123\""));
        assert_eq!(parsed.content_type.as_deref(), Some("video/mp4"));
    }

    #[tokio::test]
    async fn get_range_returns_requested_slice() {
        use crate::protocol::encode_get_range_body;
        use axum::extract::Request;
        use axum::http::{header, Response};

        // Upstream that honors Range requests on a fixed 4 KB body.
        let app = Router::new().route(
            "/data.bin",
            get(|req: Request| async move {
                let body: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
                let range = req
                    .headers()
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.strip_prefix("bytes="))
                    .and_then(|s| s.split_once('-'))
                    .and_then(|(b, e)| {
                        let b: usize = b.parse().ok()?;
                        let e: usize = e.parse().ok()?;
                        Some((b, e))
                    });
                if let Some((begin, end)) = range {
                    let clipped_end = end.min(body.len() - 1);
                    let slice = body[begin..=clipped_end].to_vec();
                    Response::builder()
                        .status(206)
                        .header(header::CONTENT_LENGTH, slice.len())
                        .header(
                            header::CONTENT_RANGE,
                            format!("bytes {begin}-{clipped_end}/{}", body.len()),
                        )
                        .body(axum::body::Body::from(slice))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(200)
                        .header(header::CONTENT_LENGTH, body.len())
                        .body(axum::body::Body::from(body))
                        .unwrap()
                }
            }),
        );
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(upstream, app).await.unwrap();
        });

        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();

        // Request bytes 1000..=1099 (100 bytes)
        let body = encode_get_range_body(1000, 100);
        write_request(
            &mut stream,
            "GET_RANGE",
            &format!("http://127.0.0.1:{upstream_port}/data.bin"),
            &body,
        )
        .await;

        let (status, response_body) = read_response(&mut stream).await;
        assert_eq!(status, 206);
        assert_eq!(response_body.len(), 100);
        let expected: Vec<u8> = (1000..1100).map(|i| (i % 251) as u8).collect();
        assert_eq!(response_body, expected);
    }

    /// adversarial: begin + length が u64 を overflow する入力を弾く。
    /// `saturating_add` で値が丸まると意味不明な Range ヘッダが upstream に
    /// 流れてしまうため、handler 側で明示的に reject する経路を検証。
    #[tokio::test]
    async fn get_range_rejects_overflow_begin_plus_length() {
        use crate::protocol::encode_get_range_body;

        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();

        // begin=u64::MAX, length=2 → u64 overflow
        let body = encode_get_range_body(u64::MAX, 2);
        write_request(
            &mut stream,
            "GET_RANGE",
            "http://example.com/x.bin",
            &body,
        )
        .await;
        let (status, msg) = read_response(&mut stream).await;
        assert_eq!(status, 0, "overflow must be proxy-internal reject");
        let msg = String::from_utf8(msg).unwrap();
        assert!(
            msg.contains("overflows"),
            "error message should mention overflow, got: {msg}"
        );
    }

    /// length=0 の GET_RANGE は upstream に行かず空応答を返す (early return パス)。
    /// `end_inclusive` 計算の `saturating_sub(1)` 罠を踏まないことを保証する。
    #[tokio::test]
    async fn get_range_zero_length_returns_empty_without_upstream() {
        use crate::protocol::encode_get_range_body;

        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();

        let body = encode_get_range_body(1000, 0);
        write_request(
            &mut stream,
            "GET_RANGE",
            // upstream URL は実際には到達できなくてもよい
            "http://127.0.0.1:1/should-not-be-called",
            &body,
        )
        .await;
        let (status, response_body) = read_response(&mut stream).await;
        assert_eq!(status, 200, "length=0 should succeed");
        assert!(response_body.is_empty(), "length=0 returns empty body");
    }

    #[tokio::test]
    async fn get_range_rejects_invalid_body_size() {
        let proxy = start_proxy().await;
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy}"))
            .await
            .unwrap();
        // 16 byte ではなく 8 byte の body を送る → proxy 内部エラー (status 0)
        write_request(
            &mut stream,
            "GET_RANGE",
            "http://example.com/data.bin",
            &[0u8; 8],
        )
        .await;
        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 0);
        assert!(String::from_utf8(body).unwrap().contains("Invalid GET_RANGE"));
    }

    #[tokio::test]
    async fn chunked_get_uses_sentinel() {
        use crate::protocol::CHUNKED_SENTINEL;
        use axum::body::Body;
        use axum::http::Response;

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
