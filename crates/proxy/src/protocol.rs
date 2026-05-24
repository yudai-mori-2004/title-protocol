// SPDX-License-Identifier: Apache-2.0

//! # Length-prefixed wire protocol
//!
//! Spec §5.2 — TEE content fetch (proxy-mediated transport)
//!
//! Used between the Enclave-side `ProxyContentFetcher` and this host-side
//! forwarder. Plain byte stream, no framing beyond `u32` length prefixes;
//! one request = one connection so we don't need a request id.
//!
//! ## TEE → Proxy
//! ```text
//! [4B u32 BE: method_len][method utf8]
//! [4B u32 BE: url_len   ][url utf8   ]
//! [4B u32 BE: body_len  ][body bytes ]
//! ```
//!
//! ## Proxy → TEE
//! ```text
//! [4B u32 BE: status_code][4B u32 BE: body_len][body bytes]
//! ```
//!
//! Status `0` is reserved for proxy-internal errors (network failure,
//! timeout, decode failure). HTTP status codes from the upstream server
//! pass through unchanged.

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
) -> std::io::Result<String> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub async fn read_bytes_async<R: tokio::io::AsyncRead + Unpin>(
    r: &mut R,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
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
pub fn read_string_sync(r: &mut impl std::io::Read) -> std::io::Result<String> {
    let len = read_u32_sync(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(all(target_os = "linux", feature = "vendor-aws"))]
pub fn read_bytes_sync(r: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let len = read_u32_sync(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}
