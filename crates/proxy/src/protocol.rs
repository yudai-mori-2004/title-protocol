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
//! The end-of-stream marker disambiguates clean termination from a
//! proxy-side cap hit: `0` means the upstream finished, `CHUNKED_TRUNCATED`
//! means the proxy stopped reading after `MAX_RESPONSE_BYTES`. The TEE must
//! treat the second case as a fetch failure rather than a complete body.
//!
//! Status `0` is reserved for proxy-internal errors (network failure,
//! timeout, decode failure). HTTP status codes from the upstream pass
//! through unchanged.

/// Sentinel value in the `body_len` field that puts the response into
/// chunked-stream mode. Picked at the top of `u32` range so a real
/// `Content-Length` could never collide (real bodies are capped well below
/// 4 GiB by `MAX_RESPONSE_BYTES`).
pub const CHUNKED_SENTINEL: u32 = u32::MAX;

/// End-of-stream marker (in place of the normal `0u32`) signalling that the
/// proxy hit `MAX_RESPONSE_BYTES` before the upstream finished. Distinct
/// from `0` so the TEE can fail the fetch instead of silently accepting a
/// truncated body. Stays inside the chunked stream: only valid where a
/// chunk length is expected, never as a content length.
pub const CHUNKED_TRUNCATED: u32 = u32::MAX;

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

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
pub fn read_u32_sync(r: &mut impl std::io::Read) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
pub fn read_string_sync(r: &mut impl std::io::Read, max_len: usize) -> std::io::Result<String> {
    let buf = read_bytes_sync(r, max_len)?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
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
