// SPDX-License-Identifier: Apache-2.0

//! # HTTP転送ハンドラ
//!
//! 仕様書 §6.4
//!
//! TEEから受け取ったHTTPリクエストを外部に転送し、レスポンスを返す。
//! GETレスポンスはストリーミングで返却し、大きなファイルでもproxy側の
//! メモリを消費しない。

use crate::protocol;

/// TEEから受け取ったHTTPリクエストを外部に転送し、ストリーミングで返す。
/// 仕様書 §6.4
///
/// GETレスポンスはContent-Lengthからbody_lenを先に送信し、
/// その後チャンク単位でストリーミング転送する。
/// POSTレスポンスは通常サイズのため一括読み込み。
pub async fn forward_http_streaming<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    method: &str,
    url: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(std::io::Error::other)?;

    let result = match method {
        "GET" => client.get(url).send().await,
        "POST" => {
            client
                .post(url)
                .header("Content-Type", "application/json")
                .body(body.to_vec())
                .send()
                .await
        }
        other => {
            tracing::error!("未サポートのHTTPメソッド: {}", other);
            let msg = format!("Unsupported method: {}", other).into_bytes();
            w.write_all(&400u32.to_be_bytes()).await?;
            w.write_all(&(msg.len() as u32).to_be_bytes()).await?;
            w.write_all(&msg).await?;
            w.flush().await?;
            return Ok(());
        }
    };

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16() as u32;

            if method == "GET" && status == 200 {
                // Streaming: send Content-Length first, then stream body chunks.
                // Proxy memory usage is O(chunk_size), not O(file_size).
                let content_length = resp.content_length().unwrap_or(0);
                tracing::info!(
                    "HTTP GET streaming: status={}, content_length={} bytes",
                    status,
                    content_length
                );

                w.write_all(&status.to_be_bytes()).await?;
                w.write_all(&(content_length as u32).to_be_bytes()).await?;

                let mut byte_stream = resp.bytes_stream();
                let mut total_written: u64 = 0;
                while let Some(chunk_result) = byte_stream.next().await {
                    let chunk = chunk_result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                    w.write_all(&chunk).await?;
                    total_written += chunk.len() as u64;
                }
                w.flush().await?;

                if total_written != content_length && content_length > 0 {
                    tracing::warn!(
                        "Streamed {} bytes but Content-Length was {}",
                        total_written,
                        content_length
                    );
                }
            } else {
                // Non-streaming: small responses (POST results, errors)
                let body_bytes = resp.bytes().await.unwrap_or_default().to_vec();
                tracing::info!(
                    "HTTP転送完了: status={}, body={} bytes",
                    status,
                    body_bytes.len()
                );
                w.write_all(&status.to_be_bytes()).await?;
                w.write_all(&(body_bytes.len() as u32).to_be_bytes()).await?;
                w.write_all(&body_bytes).await?;
                w.flush().await?;
            }
        }
        Err(e) => {
            tracing::error!("HTTPリクエスト失敗: {}", e);
            let msg = format!("Proxy error: {}", e).into_bytes();
            w.write_all(&500u32.to_be_bytes()).await?;
            w.write_all(&(msg.len() as u32).to_be_bytes()).await?;
            w.write_all(&msg).await?;
            w.flush().await?;
        }
    }

    Ok(())
}

/// TCP経由の接続を処理する（開発環境 / テスト用）。
/// 仕様書 §6.4
#[cfg(any(not(target_os = "linux"), test))]
pub async fn handle_tcp_connection(mut stream: tokio::net::TcpStream) {
    let method = match protocol::read_string_async(&mut stream).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("メソッド読み取りエラー: {}", e);
            return;
        }
    };
    let url = match protocol::read_string_async(&mut stream).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("URL読み取りエラー: {}", e);
            return;
        }
    };
    let body = match protocol::read_bytes_async(&mut stream).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("ボディ読み取りエラー: {}", e);
            return;
        }
    };

    tracing::info!("{} {} (body: {} bytes)", method, url, body.len());

    if let Err(e) = forward_http_streaming(&mut stream, &method, &url, &body).await {
        tracing::error!("ストリーミング転送エラー: {}", e);
    }
}

/// vsock接続を処理する（Linux専用）。
/// 仕様書 §6.4
///
/// リクエスト受信はブロッキング（vsock read）、
/// レスポンス送信はストリーミング（ブロッキングwriteをAsyncWriteでラップ）。
#[cfg(target_os = "linux")]
pub async fn handle_vsock_connection(stream: vsock::VsockStream) {
    // vsockストリームからリクエストを読み取り（ブロッキング）
    let result = tokio::task::spawn_blocking({
        let stream_clone = stream.try_clone().expect("vsock clone failed");
        move || {
            let mut s = stream_clone;
            let method = protocol::read_string_sync(&mut s)?;
            let url = protocol::read_string_sync(&mut s)?;
            let body = protocol::read_bytes_sync(&mut s)?;
            Ok::<_, std::io::Error>((method, url, body))
        }
    })
    .await;

    let (method, url, body) = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::error!("リクエスト読み取りエラー: {}", e);
            return;
        }
        Err(e) => {
            tracing::error!("spawn_blockingエラー: {}", e);
            return;
        }
    };

    tracing::info!("{} {} (body: {} bytes)", method, url, body.len());

    // ストリーミングレスポンス送信
    let mut writer = tokio::io::BufWriter::new(vsock_async::VsockWriter(stream));
    if let Err(e) = forward_http_streaming(&mut writer, &method, &url, &body).await {
        tracing::error!("ストリーミング転送エラー: {}", e);
    }
}

/// vsock::VsockStreamをtokio AsyncWriteとしてラップ。
#[cfg(target_os = "linux")]
mod vsock_async {
    use std::io::Write;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::AsyncWrite;

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
            Poll::Ready(Ok(()))
        }
    }

    unsafe impl Send for VsockWriter {}
}
