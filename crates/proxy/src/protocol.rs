// SPDX-License-Identifier: Apache-2.0

//! # Length-prefixed wire protocol
//!
//! Spec §5.2 — TEE content fetch (proxy-mediated transport)
//!
//! Used between the Enclave-side `ProxyContentFetcher` and this host-side
//! forwarder. One request per connection — no request id, no keep-alive.
//! Sending a second request on the same connection is undefined behavior.
//!
//! ## TEE → Proxy (request)
//! ```text
//! [4B u32 BE: method_len][method utf8]
//! [4B u32 BE: url_len   ][url utf8   ]
//! [4B u32 BE: body_len  ][body bytes ]
//! ```
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
/// (`u32::MAX - 1`) を選んでいる。両者が同値だった頃は wire 上で同じ
/// 4 バイトが位置で意味を変える設計だったが、将来 chunk_len 位置で
/// SENTINEL を別目的に使う拡張を入れた場合に silent regression が起きる
/// 構造的リスクがあったため、Round 3 で別値に分離した。real chunk_len の
/// 上限は `MAX_WIRE_CHUNK_BYTES = 4 MiB` で、`u32::MAX - 1` と十分離れる。
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
pub const MAX_RESPONSE_BYTES: u64 = 100 * 1024 * 1024;

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
