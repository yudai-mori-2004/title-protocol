// SPDX-License-Identifier: Apache-2.0

//! Spec §5.2 — HTTP forwarder between the Enclave and the public network.
//! GET responses are streamed so RSS stays bounded on large C2PA media.
//! TLS is terminated here; integrity comes from the C2PA signature, not
//! the transport (Spec §5.2).

use crate::protocol::{self, CHUNKED_SENTINEL, CHUNKED_TRUNCATED, MAX_RESPONSE_BYTES};

const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 120;
const PROXY_ERROR_STATUS: u32 = 0;
const STREAM_CHUNK_LIMIT: u32 = 4 * 1024 * 1024;

fn env_secs(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Forward one request to its upstream URL, streaming the response back
/// through `w` using the length-prefixed wire format.
pub async fn forward_http_streaming<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    method: &str,
    url: &str,
    body: &[u8],
) -> std::io::Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let started = std::time::Instant::now();
    let upstream_host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();

    let connect_timeout =
        std::time::Duration::from_secs(env_secs("PROXY_CONNECT_TIMEOUT_SECS", DEFAULT_CONNECT_TIMEOUT_SECS));
    let total_timeout =
        std::time::Duration::from_secs(env_secs("PROXY_REQUEST_TIMEOUT_SECS", DEFAULT_TOTAL_TIMEOUT_SECS));

    let client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(total_timeout)
        .build()
        .map_err(std::io::Error::other)?;

    // Spec §5.2 — proxy only forwards GET (content fetch) and POST (Solana
    // RPC submission). Limiting the set shrinks the attack surface.
    let request_result = match method {
        "GET" => client.get(url).send().await,
        "POST" => client.post(url).body(body.to_vec()).send().await,
        other => {
            tracing::warn!(method = other, upstream_host = %upstream_host, "rejecting unsupported HTTP method");
            let msg = format!("Unsupported method: {other}").into_bytes();
            write_error(w, 400, &msg).await?;
            return shutdown_write(w).await;
        }
    };

    let response = match request_result {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                err = format!("{e:#}"),
                upstream_host = %upstream_host,
                duration_ms = started.elapsed().as_millis() as u64,
                "upstream HTTP request failed",
            );
            let msg = format!("Proxy error: {e}").into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
    };

    let status = response.status().as_u16() as u32;

    if method == "GET" && status == 200 {
        let content_length = response.content_length();

        // Reject upstream bodies that don't fit our forwarding budget
        // outright — better to fail fast than start a multi-GiB stream we
        // cannot finish.
        if let Some(len) = content_length {
            if len > MAX_RESPONSE_BYTES {
                tracing::warn!(content_length = len, max = MAX_RESPONSE_BYTES, upstream_host = %upstream_host, "response too large");
                let msg = format!("response too large: {len} > {MAX_RESPONSE_BYTES}").into_bytes();
                write_error(w, PROXY_ERROR_STATUS, &msg).await?;
                return shutdown_write(w).await;
            }
        }

        let mut stream = response.bytes_stream();

        if let Some(len) = content_length {
            // Known-length path: header carries the byte count, body follows
            // verbatim. `len` already fits in u32 (checked against
            // MAX_RESPONSE_BYTES = 100 MiB above).
            w.write_all(&status.to_be_bytes()).await?;
            w.write_all(&(len as u32).to_be_bytes()).await?;
            let mut written: u64 = 0;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(std::io::Error::other)?;
                if written.saturating_add(chunk.len() as u64) > len {
                    tracing::warn!(written, len, "upstream exceeded Content-Length, truncating");
                    let remaining = (len - written) as usize;
                    if remaining > 0 {
                        w.write_all(&chunk[..remaining]).await?;
                    }
                    written = len;
                    break;
                }
                w.write_all(&chunk).await?;
                written += chunk.len() as u64;
            }
            if written < len {
                tracing::warn!(written, content_length = len, "upstream sent fewer bytes than Content-Length");
            }
            w.flush().await?;
            tracing::info!(url, status, content_length = len, duration_ms = started.elapsed().as_millis() as u64, upstream_host = %upstream_host, "streamed GET");
        } else {
            // Unknown-length path (e.g. Transfer-Encoding: chunked):
            // use the sentinel framing so the TEE can drive a loop.
            w.write_all(&status.to_be_bytes()).await?;
            w.write_all(&CHUNKED_SENTINEL.to_be_bytes()).await?;
            let mut total: u64 = 0;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(std::io::Error::other)?;
                total = total.saturating_add(chunk.len() as u64);
                if total > MAX_RESPONSE_BYTES {
                    tracing::warn!(total, max = MAX_RESPONSE_BYTES, upstream_host = %upstream_host, "chunked response exceeded budget");
                    // Signal truncation with a dedicated marker so the TEE
                    // surfaces a fetch error instead of treating the partial
                    // body as a complete response.
                    w.write_all(&CHUNKED_TRUNCATED.to_be_bytes()).await?;
                    w.flush().await?;
                    return shutdown_write(w).await;
                }
                for piece in chunk.chunks(STREAM_CHUNK_LIMIT as usize) {
                    w.write_all(&(piece.len() as u32).to_be_bytes()).await?;
                    w.write_all(piece).await?;
                }
            }
            w.write_all(&0u32.to_be_bytes()).await?; // end marker
            w.flush().await?;
            tracing::info!(url, status, body_len = total, duration_ms = started.elapsed().as_millis() as u64, upstream_host = %upstream_host, "streamed chunked GET");
        }
        shutdown_write(w).await
    } else {
        let body_bytes = match response.bytes().await {
            Ok(b) => b.to_vec(),
            Err(e) => {
                tracing::error!(
                    err = format!("{e:#}"),
                    upstream_host = %upstream_host,
                    duration_ms = started.elapsed().as_millis() as u64,
                    "upstream body read failed",
                );
                let msg = format!("Proxy body read failed: {e}").into_bytes();
                write_error(w, PROXY_ERROR_STATUS, &msg).await?;
                return shutdown_write(w).await;
            }
        };
        if body_bytes.len() as u64 > MAX_RESPONSE_BYTES {
            tracing::warn!(body_len = body_bytes.len(), max = MAX_RESPONSE_BYTES, upstream_host = %upstream_host, "response too large");
            let msg = format!("response too large: {} > {MAX_RESPONSE_BYTES}", body_bytes.len()).into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
        tracing::info!(url, status, body_len = body_bytes.len(), duration_ms = started.elapsed().as_millis() as u64, upstream_host = %upstream_host, "forwarded");
        w.write_all(&status.to_be_bytes()).await?;
        w.write_all(&(body_bytes.len() as u32).to_be_bytes()).await?;
        w.write_all(&body_bytes).await?;
        w.flush().await?;
        shutdown_write(w).await
    }
}

async fn write_error<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    status: u32,
    body: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    w.write_all(&status.to_be_bytes()).await?;
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await?;
    Ok(())
}

async fn shutdown_write<W: tokio::io::AsyncWrite + Unpin>(w: &mut W) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let _ = w.shutdown().await;
    Ok(())
}

// ----------------------------------------------------------------------------
// TCP connection handler — used by dev/test mode and by the integration tests
// in main.rs. The vsock handler lives in main.rs because it depends on
// platform-gated `vsock` types we don't want to leak into this module.
// ----------------------------------------------------------------------------

pub async fn handle_tcp_connection(mut stream: tokio::net::TcpStream) {
    use crate::protocol::{MAX_METHOD_BYTES, MAX_REQUEST_BODY_BYTES, MAX_URL_BYTES};

    let method = match protocol::read_string_async(&mut stream, MAX_METHOD_BYTES).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "failed to read method");
            return;
        }
    };
    let url = match protocol::read_string_async(&mut stream, MAX_URL_BYTES).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "failed to read url");
            return;
        }
    };
    let body = match protocol::read_bytes_async(&mut stream, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "failed to read body");
            return;
        }
    };

    tracing::info!(method, url, body_len = body.len(), "request received");

    if let Err(e) = forward_http_streaming(&mut stream, &method, &url, &body).await {
        tracing::error!(error = %e, "forwarder write failed");
    }
}
