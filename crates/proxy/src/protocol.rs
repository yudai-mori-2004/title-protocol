// SPDX-License-Identifier: Apache-2.0

//! # Length-prefixed wire protocol
//!
//! Spec §5.2, §4.3 — TEE content fetch (proxy-mediated transport)
//!
//! Used between the Enclave-side `ProxyContentFetcher` and this host-side
//! forwarder. One request per connection — no request id, no keep-alive.
//! Sending a second request on the same connection is undefined behavior.
//!
//! ## TEE → Proxy (request)
//!
//! ```text
//! [4B u32 BE: method_len][method utf8]
//! [4B u32 BE: url_len   ][url utf8   ]
//! [4B u32 BE: body_len  ][body bytes ]
//! ```
//!
//! 対応メソッド (proxy 側 method whitelist):
//! - `GET`        — 通常の content fetch (body は空)
//! - `POST`       — Solana RPC 等の送信 (body は payload)
//! - `PROBE`      — Range Request 対応確認用 (§4.3)。proxy は実際には
//!                  `GET Range: bytes=0-0` を upstream に投げて 206 / Content-Range
//!                  から構造化メタデータを抽出する。R2 / S3 の SigV4 presigned GET
//!                  URL は HTTP method が署名対象なので HTTP HEAD が SignatureDoesNotMatch
//!                  で落ちるが、`Range` ヘッダは signed header に含まれないため
//!                  Range GET なら presigned GET URL でも署名が通る。
//! - `GET_RANGE`  — Range Request 本体 (body は `[8B u64 BE: begin][8B u64 BE: length]`、§4.3)
//!
//! ## Proxy → TEE (response)
//!
//! Either a single framed body:
//! ```text
//! [4B u32 BE: status_code][4B u32 BE: body_len (< CHUNKED_SENTINEL)][body bytes]
//! ```
//!
//! Or a chunked stream when the upstream `Content-Length` is unknown (e.g.
//! `Transfer-Encoding: chunked`):
//! ```text
//! [4B u32 BE: status_code][4B u32 BE: CHUNKED_SENTINEL]
//! [4B u32 BE: chunk_len][chunk bytes] ...repeated...
//! [4B u32 BE: 0 or CHUNKED_TRUNCATED]   // end-of-stream marker
//! ```
//!
//! ## PROBE response body format
//!
//! `PROBE` requests return a structured body that the TEE side parses to
//! decide whether Range Request streaming is feasible. status_code は upstream の
//! HTTP ステータス (206 = Range 対応、200 = Range 非対応、それ以外は upstream error)。
//! body は固定レイアウト:
//!
//! ```text
//! [8B u64 BE: content_length]       // 206 時: Content-Range の分母。それ以外: 0
//! [1B u8: accepts_ranges]            // 1 = 206 を確認、0 = それ以外
//! [4B u32 BE: etag_len][etag utf8]
//! [4B u32 BE: content_type_len][content_type utf8]
//! ```
//!
//! `etag_len` / `content_type_len` は 0 でも構わない (= ヘッダ未送出)。
//! `accepts_ranges = 0` の場合、TEE 側は full fetch にフォールバックする。
//!
//! ### chunk_len の解釈空間
//!
//! - `0`                                = clean EOF (upstream が正常終了)
//! - `1..=MAX_WIRE_CHUNK_BYTES`         = real chunk length
//! - `CHUNKED_TRUNCATED` (= `u32::MAX - 1`) = proxy 側が `MAX_RESPONSE_BYTES`
//!   を超えたため受信を打ち切ったマーカー。TEE はこれを fetch 失敗として
//!   surface する
//! - `CHUNKED_SENTINEL` (= `u32::MAX`)  = body_len フィールド (status 直後)
//!   に限り出現する。chunked モード突入を示す。`chunk_len` 位置には決して
//!   出現しない (= 万一読み出しを誤って sentinel と truncation が同値だった
//!   時代の silent regression を避けるため、両者は別ビットパターン)
//!
//! Status `0` is reserved for proxy-internal errors (network failure,
//! timeout, decode failure, unsupported method 等 proxy 内部由来エラー)。
//! HTTP status codes from the upstream pass through unchanged.

/// Sentinel value in the `body_len` field (status 直後位置) that puts the
/// response into chunked-stream mode. `u32` レンジの最上位なので real
/// `Content-Length` とは衝突しない (real bodies are capped well below
/// 4 GiB by `MAX_RESPONSE_BYTES`)。
pub const CHUNKED_SENTINEL: u32 = u32::MAX;

/// End-of-stream marker (in place of the normal `0u32`) signalling that the
/// proxy hit `MAX_RESPONSE_BYTES` before the upstream finished. Distinct
/// from `0` so the TEE can fail the fetch instead of silently accepting a
/// truncated body. Stays inside the chunked stream: only valid where a
/// chunk length is expected, never as a content length.
///
/// 値は `CHUNKED_SENTINEL` (= `u32::MAX`) とは**別ビットパターン**
/// (`u32::MAX - 1`) を選ぶ。両者が同値だと wire 上で同じ 4 バイトが位置で
/// 意味を変える設計になり、chunk_len 位置で SENTINEL を別目的に使う将来拡張
/// で silent regression を起こすため。real chunk_len の上限は
/// `MAX_WIRE_CHUNK_BYTES = 4 MiB` で、`u32::MAX - 1` と十分離れる。
pub const CHUNKED_TRUNCATED: u32 = u32::MAX - 1;

/// 1 chunk あたりの最大バイト数。proxy 側は `bytes_stream` から受け取った
/// piece をこの上限で分割して書き出す。TEE 側は `read_chunked_body` で
/// `chunk_len > MAX_WIRE_CHUNK_BYTES` を proxy 故障として弾いてよい。
pub const MAX_WIRE_CHUNK_BYTES: u32 = 4 * 1024 * 1024;

/// Maximum byte length accepted for the proxy request `method` field.
pub const MAX_METHOD_BYTES: usize = 16;
/// Maximum byte length accepted for the proxy request `url` field.
pub const MAX_URL_BYTES: usize = 8 * 1024;
/// Maximum byte length accepted for the proxy request `body` field.
pub const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Maximum total response body the proxy is willing to forward.
/// Raised to 2 GiB to support full-length / high-resolution video as
/// single-request content (see `tee::main` `MAX_CONTENT_BYTES`). The
/// enclave still applies its own cap via `ProxyContentFetcher::max_body_bytes`,
/// so this is just the upstream-fetch budget; nothing forces the enclave
/// to actually accept this much.
pub const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// `PROBE` 応答 body の固定ヘッダ部 (content_length:u64 + accepts_ranges:u8) の長さ。
/// その後に `[u32 etag_len][etag][u32 ct_len][ct]` が続く。
pub const PROBE_RESPONSE_FIXED_PREFIX_BYTES: usize = 8 + 1;

/// `GET_RANGE` リクエスト body のサイズ (begin:u64 + length:u64 = 16 byte 固定)。
pub const GET_RANGE_REQUEST_BODY_BYTES: usize = 16;

/// Encode a PROBE response body into the wire format described in this module's
/// doc comment.
pub fn encode_probe_response(
    content_length: u64,
    accepts_ranges: bool,
    etag: Option<&str>,
    content_type: Option<&str>,
) -> Vec<u8> {
    let etag = etag.unwrap_or("");
    let ct = content_type.unwrap_or("");
    let mut buf = Vec::with_capacity(
        PROBE_RESPONSE_FIXED_PREFIX_BYTES + 4 + etag.len() + 4 + ct.len(),
    );
    buf.extend_from_slice(&content_length.to_be_bytes());
    buf.push(if accepts_ranges { 1 } else { 0 });
    buf.extend_from_slice(&(etag.len() as u32).to_be_bytes());
    buf.extend_from_slice(etag.as_bytes());
    buf.extend_from_slice(&(ct.len() as u32).to_be_bytes());
    buf.extend_from_slice(ct.as_bytes());
    buf
}

/// Parsed PROBE response. Mirror of [`encode_probe_response`].
#[derive(Debug, Clone)]
pub struct ParsedProbeResponse {
    pub content_length: u64,
    pub accepts_ranges: bool,
    pub etag: Option<String>,
    pub content_type: Option<String>,
}

/// Decode a PROBE response body (as produced by [`encode_probe_response`]).
pub fn decode_probe_response(body: &[u8]) -> Result<ParsedProbeResponse, &'static str> {
    if body.len() < PROBE_RESPONSE_FIXED_PREFIX_BYTES {
        return Err("PROBE response too short for fixed prefix");
    }
    let content_length =
        u64::from_be_bytes(body[..8].try_into().expect("8 bytes => [u8; 8]"));
    let accepts_ranges = body[8] != 0;
    let mut pos = PROBE_RESPONSE_FIXED_PREFIX_BYTES;

    if body.len() < pos + 4 {
        return Err("PROBE response truncated before etag_len");
    }
    let etag_len = u32::from_be_bytes(
        body[pos..pos + 4]
            .try_into()
            .expect("4 bytes => [u8; 4]"),
    ) as usize;
    pos += 4;
    if body.len() < pos + etag_len {
        return Err("PROBE response truncated before etag bytes");
    }
    let etag = if etag_len == 0 {
        None
    } else {
        Some(
            std::str::from_utf8(&body[pos..pos + etag_len])
                .map_err(|_| "PROBE response etag not utf8")?
                .to_string(),
        )
    };
    pos += etag_len;

    if body.len() < pos + 4 {
        return Err("PROBE response truncated before content_type_len");
    }
    let ct_len = u32::from_be_bytes(
        body[pos..pos + 4]
            .try_into()
            .expect("4 bytes => [u8; 4]"),
    ) as usize;
    pos += 4;
    if body.len() < pos + ct_len {
        return Err("PROBE response truncated before content_type bytes");
    }
    let content_type = if ct_len == 0 {
        None
    } else {
        Some(
            std::str::from_utf8(&body[pos..pos + ct_len])
                .map_err(|_| "PROBE response content_type not utf8")?
                .to_string(),
        )
    };

    Ok(ParsedProbeResponse {
        content_length,
        accepts_ranges,
        etag,
        content_type,
    })
}

/// `Content-Range: bytes 0-0/12345` → `Some(12345)`。`*` (= 全長不明) または
/// parse 失敗時は `None`。RFC 7233 §4.2 準拠の最小実装で、`bytes <start>-<end>/<total>`
/// の `/` 以降だけを見る。
pub fn parse_content_range_total(value: &str) -> Option<u64> {
    let after_slash = value.rsplit_once('/').map(|(_, t)| t.trim())?;
    if after_slash == "*" {
        None
    } else {
        after_slash.parse().ok()
    }
}

/// Encode a GET_RANGE request body: `[u64 begin][u64 length]`.
pub fn encode_get_range_body(begin: u64, length: u64) -> [u8; GET_RANGE_REQUEST_BODY_BYTES] {
    let mut buf = [0u8; GET_RANGE_REQUEST_BODY_BYTES];
    buf[..8].copy_from_slice(&begin.to_be_bytes());
    buf[8..].copy_from_slice(&length.to_be_bytes());
    buf
}

/// Decode a GET_RANGE request body.
pub fn decode_get_range_body(body: &[u8]) -> Result<(u64, u64), &'static str> {
    if body.len() != GET_RANGE_REQUEST_BODY_BYTES {
        return Err("GET_RANGE body must be exactly 16 bytes");
    }
    let begin = u64::from_be_bytes(body[..8].try_into().expect("8 bytes => [u8; 8]"));
    let length = u64::from_be_bytes(body[8..].try_into().expect("8 bytes => [u8; 8]"));
    Ok((begin, length))
}

// ----------------------------------------------------------------------------
// Async I/O — used on the TCP code path (dev/test) and on the vsock writer
// side (responses stream back through a tokio AsyncWrite wrapper).
// ----------------------------------------------------------------------------

pub async fn read_u32_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<u32> {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).await?;
    Ok(u32::from_be_bytes(buf))
}

pub async fn read_string_async<R: tokio::io::AsyncRead + Unpin>(
    r: &mut R,
    max_len: usize,
) -> std::io::Result<String> {
    let buf = read_bytes_async(r, max_len).await?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub async fn read_bytes_async<R: tokio::io::AsyncRead + Unpin>(
    r: &mut R,
    max_len: usize,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
    if len > max_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("length {len} exceeds maximum {max_len}"),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

// ----------------------------------------------------------------------------
// Sync I/O — used by the vsock accept loop, which runs on a blocking
// thread because the `vsock` crate's stream is `std::io::Read`/`Write`.
// ----------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn read_u32_sync(r: &mut impl std::io::Read) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

#[cfg(target_os = "linux")]
pub fn read_string_sync(r: &mut impl std::io::Read, max_len: usize) -> std::io::Result<String> {
    let buf = read_bytes_sync(r, max_len)?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(target_os = "linux")]
pub fn read_bytes_sync(r: &mut impl std::io::Read, max_len: usize) -> std::io::Result<Vec<u8>> {
    let len = read_u32_sync(r)? as usize;
    if len > max_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("length {len} exceeds maximum {max_len}"),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

// ----------------------------------------------------------------------------
// Tests for PROBE / GET_RANGE encoding
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_response_roundtrip_with_all_headers() {
        let encoded = encode_probe_response(
            1_073_741_824,
            true,
            Some("\"abc123\""),
            Some("video/mp4"),
        );
        let parsed = decode_probe_response(&encoded).expect("decode");
        assert_eq!(parsed.content_length, 1_073_741_824);
        assert!(parsed.accepts_ranges);
        assert_eq!(parsed.etag.as_deref(), Some("\"abc123\""));
        assert_eq!(parsed.content_type.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn probe_response_roundtrip_without_optional_headers() {
        let encoded = encode_probe_response(42, false, None, None);
        let parsed = decode_probe_response(&encoded).expect("decode");
        assert_eq!(parsed.content_length, 42);
        assert!(!parsed.accepts_ranges);
        assert!(parsed.etag.is_none());
        assert!(parsed.content_type.is_none());
    }

    #[test]
    fn probe_response_rejects_truncated_input() {
        let encoded = encode_probe_response(100, true, Some("e"), Some("ct"));
        // 切り詰めて 1 byte 落とす → decode 失敗
        assert!(decode_probe_response(&encoded[..encoded.len() - 1]).is_err());
        assert!(decode_probe_response(&[]).is_err());
        assert!(decode_probe_response(&[0u8; PROBE_RESPONSE_FIXED_PREFIX_BYTES]).is_err());
    }

    #[test]
    fn content_range_total_parses_206_form() {
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(parse_content_range_total("bytes 100-199/2048"), Some(2048));
    }

    #[test]
    fn content_range_total_handles_unknown_size_and_garbage() {
        assert_eq!(parse_content_range_total("bytes 0-0/*"), None);
        assert_eq!(parse_content_range_total("garbage"), None);
        assert_eq!(parse_content_range_total(""), None);
        assert_eq!(parse_content_range_total("bytes 0-0/abc"), None);
    }

    #[test]
    fn get_range_body_roundtrip() {
        let encoded = encode_get_range_body(2048, 4096);
        assert_eq!(encoded.len(), GET_RANGE_REQUEST_BODY_BYTES);
        let (begin, length) = decode_get_range_body(&encoded).expect("decode");
        assert_eq!(begin, 2048);
        assert_eq!(length, 4096);
    }

    #[test]
    fn get_range_body_rejects_wrong_size() {
        assert!(decode_get_range_body(&[]).is_err());
        assert!(decode_get_range_body(&[0u8; 8]).is_err());
        assert!(decode_get_range_body(&[0u8; 24]).is_err());
    }
}
