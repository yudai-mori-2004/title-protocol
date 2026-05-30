// SPDX-License-Identifier: Apache-2.0

//! Spec §5.2 — HTTP forwarder between the Enclave and the public network.
//! GET responses are streamed so RSS stays bounded on large C2PA media.
//! TLS is terminated here; integrity comes from the C2PA signature, not
//! the transport (Spec §5.2).

use crate::protocol::{
    self, decode_get_range_body, encode_probe_response, parse_content_range_total,
    CHUNKED_SENTINEL, CHUNKED_TRUNCATED, MAX_RESPONSE_BYTES, MAX_WIRE_CHUNK_BYTES,
};

const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 120;
const PROXY_ERROR_STATUS: u32 = 0;

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

    let connect_timeout = std::time::Duration::from_secs(env_secs(
        "PROXY_CONNECT_TIMEOUT_SECS",
        DEFAULT_CONNECT_TIMEOUT_SECS,
    ));
    let total_timeout = std::time::Duration::from_secs(env_secs(
        "PROXY_REQUEST_TIMEOUT_SECS",
        DEFAULT_TOTAL_TIMEOUT_SECS,
    ));

    let client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(total_timeout)
        .build()
        .map_err(std::io::Error::other)?;

    // Spec §5.2, §4.3 — proxy only forwards GET / POST (existing) と
    // PROBE / GET_RANGE (Range Request streaming 用、§4.3 で追加)。
    // method whitelist を絞ることで attack surface を最小化する。
    //
    // PROBE は Range Request 対応の事前確認。proxy は upstream に
    // `GET Range: bytes=0-0` を投げ、206 + Content-Range の分母から
    // 全長を取り出して構造化応答で返す。HTTP HEAD を使わない理由:
    // R2 / S3 の SigV4 presigned GET URL は HTTP method が署名対象なので
    // HEAD だと SignatureDoesNotMatch (= 403) で落ちる。一方 `Range` ヘッダは
    // signed header に含まれないため、同じ署名済 GET URL に Range を付けて
    // 投げる経路は 206 で通る。
    //
    // GET_RANGE は body に `[u64 begin][u64 length]` を載せ、proxy が
    // `Range: bytes=begin-(begin+length-1)` 付きで GET を発行する。
    let request_result = match method {
        "GET" => client.get(url).send().await,
        "POST" => client.post(url).body(body.to_vec()).send().await,
        "PROBE" => {
            // PROBE は専用パスで処理 (応答ボディが HTTP body ではなく構造化メタデータ)。
            return handle_probe(w, &client, url, &upstream_host, started).await;
        }
        "GET_RANGE" => {
            return handle_get_range(w, &client, url, body, &upstream_host, started).await;
        }
        other => {
            tracing::warn!(method = other, upstream_host = %upstream_host, "rejecting unsupported HTTP method");
            let msg = format!("Unsupported method: {other}").into_bytes();
            // proxy 内部での拒否なので PROXY_ERROR_STATUS (= 0) を使う。
            // 上流由来の HTTP 400 と区別できるようにすることで、TEE 側で
            // 「proxy が拒否したのか上流が拒否したのか」を切り分けられる。
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
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
                tracing::warn!(
                    written,
                    content_length = len,
                    "upstream sent fewer bytes than Content-Length"
                );
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
                for piece in chunk.chunks(MAX_WIRE_CHUNK_BYTES as usize) {
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
        // 非 GET / 非 200 経路。GET 200 と対称にストリーミング読みで
        // `MAX_RESPONSE_BYTES` を強制する。以前は `response.bytes().await` で
        // body 全体をメモリに展開してから上限チェックしていたが、これだと
        // 攻撃者制御の上流が 1 GiB を返した時点で proxy が OOM する。
        let content_length = response.content_length();
        if let Some(len) = content_length {
            if len > MAX_RESPONSE_BYTES {
                tracing::warn!(content_length = len, max = MAX_RESPONSE_BYTES, upstream_host = %upstream_host, "response too large");
                let msg =
                    format!("response too large: {len} > {MAX_RESPONSE_BYTES}").into_bytes();
                write_error(w, PROXY_ERROR_STATUS, &msg).await?;
                return shutdown_write(w).await;
            }
        }

        let mut stream = response.bytes_stream();
        let mut body_bytes: Vec<u8> = Vec::with_capacity(
            content_length
                .map(|n| n as usize)
                .unwrap_or(0)
                .min(MAX_RESPONSE_BYTES as usize),
        );
        loop {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    if body_bytes.len() as u64 + chunk.len() as u64 > MAX_RESPONSE_BYTES {
                        tracing::warn!(
                            seen = body_bytes.len() + chunk.len(),
                            max = MAX_RESPONSE_BYTES,
                            upstream_host = %upstream_host,
                            "response too large",
                        );
                        let msg = format!(
                            "response too large: > {MAX_RESPONSE_BYTES}",
                        )
                        .into_bytes();
                        write_error(w, PROXY_ERROR_STATUS, &msg).await?;
                        return shutdown_write(w).await;
                    }
                    body_bytes.extend_from_slice(&chunk);
                }
                Some(Err(e)) => {
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
                None => break,
            }
        }
        tracing::info!(url, status, body_len = body_bytes.len(), duration_ms = started.elapsed().as_millis() as u64, upstream_host = %upstream_host, "forwarded");
        w.write_all(&status.to_be_bytes()).await?;
        w.write_all(&(body_bytes.len() as u32).to_be_bytes())
            .await?;
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

/// PROBE: upstream に `GET Range: bytes=0-0` を投げ、`encode_probe_response`
/// で構造化応答を返す。HTTP HEAD は使わない (R2 / S3 の SigV4 presigned GET URL
/// は HEAD で SignatureDoesNotMatch になるため)。
///
/// 判定ロジック:
/// - status == 206: Range 対応確認。`Content-Range: bytes 0-0/<total>` の分母から
///   全長を抽出 (= `parse_content_range_total`)。`accepts_ranges = true`。
/// - status == 200: upstream が Range ヘッダを無視して全 body を返そうとした。
///   ここで body を読み始めると全体ダウンロードになるので消費せずに drop し、
///   `accepts_ranges = false` で返して TEE 側を full-fetch fallback に落とす。
/// - その他 (3xx/4xx/5xx): そのまま透過。body は空。
async fn handle_probe<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    client: &reqwest::Client,
    url: &str,
    upstream_host: &str,
    started: std::time::Instant,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;

    let response = match client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                err = format!("{e:#}"),
                upstream_host = %upstream_host,
                "PROBE request failed",
            );
            let msg = format!("Proxy PROBE error: {e}").into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
    };

    let status_code = response.status().as_u16();
    let status = status_code as u32;

    // 成功系 (= 2xx) 以外は body 空で status だけ透過。upstream が 4xx/5xx を
    // 返した場合は TEE 側で wire の non-2xx ハンドリングに任せる。
    if !response.status().is_success() {
        // body を消費せずに drop してコネクションを解放する。
        drop(response);
        w.write_all(&status.to_be_bytes()).await?;
        w.write_all(&0u32.to_be_bytes()).await?;
        w.flush().await?;
        tracing::info!(
            url,
            status,
            upstream_host = %upstream_host,
            duration_ms = started.elapsed().as_millis() as u64,
            "PROBE (non-2xx, passthrough)",
        );
        return shutdown_write(w).await;
    }

    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let (content_length, accepts_ranges) = if status_code == 206 {
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let total = content_range
            .as_deref()
            .and_then(parse_content_range_total)
            .unwrap_or(0);
        (total, true)
    } else {
        // 200 等は Range 無視 → fallback 経路に。Content-Length ヘッダは
        // 念のため拾う (= TEE 側 metric 用)。
        let cl = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        (cl, false)
    };

    // ここで明示的に drop して upstream connection を閉じる。206 でも 1 バイト
    // (= bytes=0-0 の応答 body) を読まずに切る — その分は後続の GET_RANGE で
    // 改めて取り直す。proxy 側でも追加メモリを抱えないことを保証する。
    drop(response);

    let body = encode_probe_response(
        content_length,
        accepts_ranges,
        etag.as_deref(),
        content_type.as_deref(),
    );

    w.write_all(&status.to_be_bytes()).await?;
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;

    tracing::info!(
        url,
        status,
        content_length,
        accepts_ranges,
        upstream_host = %upstream_host,
        duration_ms = started.elapsed().as_millis() as u64,
        "PROBE",
    );
    shutdown_write(w).await
}

/// GET_RANGE: body から `[u64 begin][u64 length]` を読み、`Range: bytes=begin-(begin+length-1)`
/// 付きで upstream に GET を投げ、応答をそのまま (frame として) 返す。
async fn handle_get_range<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    client: &reqwest::Client,
    url: &str,
    body: &[u8],
    upstream_host: &str,
    started: std::time::Instant,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;

    let (begin, length) = match decode_get_range_body(body) {
        Ok(v) => v,
        Err(reason) => {
            tracing::warn!(reason, "invalid GET_RANGE body");
            let msg = format!("Invalid GET_RANGE body: {reason}").into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
    };

    if length == 0 {
        // 0 byte 要求は upstream に投げず空応答で済ます。
        w.write_all(&200u32.to_be_bytes()).await?;
        w.write_all(&0u32.to_be_bytes()).await?;
        w.flush().await?;
        return shutdown_write(w).await;
    }

    if length > MAX_RESPONSE_BYTES {
        let msg = format!("GET_RANGE length {length} exceeds MAX_RESPONSE_BYTES").into_bytes();
        write_error(w, PROXY_ERROR_STATUS, &msg).await?;
        return shutdown_write(w).await;
    }

    // begin + length が u64 を overflow するケース (攻撃者が begin=u64::MAX,
    // length=1 などを送る) を弾く。`saturating_add` で値を丸めるのではなく
    // 明確に reject することで、upstream に意味不明な Range ヘッダを投げない。
    let end_exclusive = match begin.checked_add(length) {
        Some(v) => v,
        None => {
            let msg = format!("GET_RANGE begin {begin} + length {length} overflows u64")
                .into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
    };
    let end_inclusive = end_exclusive - 1; // length > 0 は上で保証済み
    let range_header = format!("bytes={begin}-{end_inclusive}");

    let response = match client
        .get(url)
        .header(reqwest::header::RANGE, &range_header)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                err = format!("{e:#}"),
                upstream_host = %upstream_host,
                "GET_RANGE upstream failed",
            );
            let msg = format!("Proxy GET_RANGE error: {e}").into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
    };

    let status = response.status().as_u16() as u32;
    let response_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("GET_RANGE body read failed: {e}").into_bytes();
            write_error(w, PROXY_ERROR_STATUS, &msg).await?;
            return shutdown_write(w).await;
        }
    };

    if response_bytes.len() as u64 > MAX_RESPONSE_BYTES {
        let msg = format!(
            "GET_RANGE upstream returned {} bytes > MAX_RESPONSE_BYTES",
            response_bytes.len()
        )
        .into_bytes();
        write_error(w, PROXY_ERROR_STATUS, &msg).await?;
        return shutdown_write(w).await;
    }

    w.write_all(&status.to_be_bytes()).await?;
    w.write_all(&(response_bytes.len() as u32).to_be_bytes()).await?;
    w.write_all(&response_bytes).await?;
    w.flush().await?;

    tracing::info!(
        url,
        status,
        range = %range_header,
        body_len = response_bytes.len(),
        upstream_host = %upstream_host,
        duration_ms = started.elapsed().as_millis() as u64,
        "GET_RANGE",
    );
    shutdown_write(w).await
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
