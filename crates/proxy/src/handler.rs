// SPDX-License-Identifier: Apache-2.0

//! # HTTP forwarder
//!
//! Spec §5.2 — TEE content fetch (proxy-mediated transport)
//!
//! Receives a `(method, url, body)` triple from the Enclave, issues the
//! corresponding HTTP/HTTPS request, and streams the response back over
//! the same length-prefixed wire protocol.
//!
//! GET responses are streamed in chunks so the proxy's RSS doesn't grow
//! with the upstream body size — important for the C2PA media files
//! Title Protocol typically processes (often 10s of MB).
//!
//! TLS termination happens here, not in the Enclave. That's deliberate:
//! Title Protocol's trust model derives integrity from the C2PA signature
//! embedded in the content, not from the transport, so this proxy seeing
//! the plaintext bytes does not weaken the protocol.

use crate::protocol;

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const PROXY_ERROR_STATUS: u32 = 0;

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

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(std::io::Error::other)?;

    let request_result = match method {
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
            tracing::warn!(method = other, "rejecting unsupported HTTP method");
            let msg = format!("Unsupported method: {other}").into_bytes();
            write_error(w, 400, &msg).await?;
            return Ok(());
        }
    };

    let response = match request_result {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "upstream HTTP request failed");
            let msg = format!("Proxy error: {e}").into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return Ok(());
        }
    };

    let status = response.status().as_u16() as u32;

    if method == "GET" && status == 200 {
        // Stream large GET bodies chunk-by-chunk.
        let content_length = response.content_length().unwrap_or(0);
        tracing::info!(
            url = url,
            status,
            content_length,
            "streaming GET response"
        );

        w.write_all(&status.to_be_bytes()).await?;
        w.write_all(&(content_length as u32).to_be_bytes()).await?;

        let mut stream = response.bytes_stream();
        let mut written: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(std::io::Error::other)?;
            w.write_all(&chunk).await?;
            written += chunk.len() as u64;
        }
        w.flush().await?;

        if content_length > 0 && written != content_length {
            tracing::warn!(
                written,
                content_length,
                "streamed byte count differs from Content-Length"
            );
        }
        Ok(())
    } else {
        // Small responses (POSTs, errors) fit comfortably in memory.
        let body_bytes = response.bytes().await.unwrap_or_default().to_vec();
        tracing::info!(url = url, status, body_len = body_bytes.len(), "forwarded");
        w.write_all(&status.to_be_bytes()).await?;
        w.write_all(&(body_bytes.len() as u32).to_be_bytes()).await?;
        w.write_all(&body_bytes).await?;
        w.flush().await?;
        Ok(())
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

// ----------------------------------------------------------------------------
// TCP connection handler — used by dev/test mode and by the integration tests
// in main.rs. The vsock handler lives in main.rs because it depends on
// platform-gated `vsock` types we don't want to leak into this module.
// ----------------------------------------------------------------------------

pub async fn handle_tcp_connection(mut stream: tokio::net::TcpStream) {
    let method = match protocol::read_string_async(&mut stream).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "failed to read method");
            return;
        }
    };
    let url = match protocol::read_string_async(&mut stream).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "failed to read url");
            return;
        }
    };
    let body = match protocol::read_bytes_async(&mut stream).await {
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
