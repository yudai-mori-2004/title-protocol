// SPDX-License-Identifier: Apache-2.0

//! # Proxy-mediated content fetcher
//!
//! Spec §5.2 — TEE content fetch (proxy-mediated transport)
//!
//! Used when the TEE runs inside a Nitro Enclave: the Enclave has no
//! network interface, so every fetch is tunneled to a `title-proxy`
//! instance on the EC2 host via vsock, which then makes the real HTTPS
//! call and streams the body back.
//!
//! The same crate works in pure-TCP mode for local development — point
//! `PROXY_ADDR` at a host:port pair instead of a vsock CID and the same
//! length-prefixed protocol carries the traffic over loopback.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::content_fetch::{detect_content_type, ContentFetcher, FetchError, FetchResponse};

const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_IO_TIMEOUT: Duration = Duration::from_secs(60);
const PROXY_INTERNAL_ERROR_STATUS: u32 = 0;

/// Where the proxy is listening.
#[derive(Debug, Clone)]
pub enum ProxyEndpoint {
    /// Loopback / dev mode. Format: `"127.0.0.1:8000"`.
    Tcp(String),
    /// Nitro Enclave production mode. CID is typically `3` (parent host).
    #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
    Vsock { cid: u32, port: u32 },
}

impl ProxyEndpoint {
    /// Parse the `PROXY_ADDR` env-var convention.
    ///
    /// - `"vsock://3:8000"` → vsock CID=3 port=8000 (production)
    /// - `"127.0.0.1:8000"` → TCP loopback (dev)
    pub fn parse(addr: &str) -> Result<Self, String> {
        if let Some(rest) = addr.strip_prefix("vsock://") {
            #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
            {
                let (cid_s, port_s) = rest
                    .split_once(':')
                    .ok_or_else(|| format!("expected vsock://CID:PORT, got {addr}"))?;
                let cid: u32 = cid_s
                    .parse()
                    .map_err(|e| format!("invalid CID in {addr}: {e}"))?;
                let port: u32 = port_s
                    .parse()
                    .map_err(|e| format!("invalid port in {addr}: {e}"))?;
                Ok(Self::Vsock { cid, port })
            }
            #[cfg(not(all(target_os = "linux", feature = "vendor-aws")))]
            {
                let _ = rest;
                Err(format!(
                    "vsock endpoints are only supported when built with `--features vendor-aws` on Linux (requested: {addr})"
                ))
            }
        } else {
            Ok(Self::Tcp(addr.to_string()))
        }
    }
}

pub struct ProxyContentFetcher {
    endpoint: ProxyEndpoint,
    max_body_bytes: usize,
}

impl ProxyContentFetcher {
    /// Default 100 MB cap, matching `HttpContentFetcher`.
    pub const DEFAULT_MAX_BODY_BYTES: usize = 100 * 1024 * 1024;

    pub fn new(endpoint: ProxyEndpoint) -> Self {
        Self::with_max_body_bytes(endpoint, Self::DEFAULT_MAX_BODY_BYTES)
    }

    pub fn with_max_body_bytes(endpoint: ProxyEndpoint, max_body_bytes: usize) -> Self {
        Self {
            endpoint,
            max_body_bytes,
        }
    }

    fn open(&self) -> Result<Box<dyn ReadWrite>, FetchError> {
        match &self.endpoint {
            ProxyEndpoint::Tcp(addr) => {
                let stream = TcpStream::connect_timeout(
                    &addr.parse().map_err(|e| FetchError::HttpError {
                        url: addr.clone(),
                        reason: format!("invalid proxy addr: {e}"),
                    })?,
                    PROXY_CONNECT_TIMEOUT,
                )
                .map_err(|e| FetchError::HttpError {
                    url: addr.clone(),
                    reason: format!("proxy connect failed: {e}"),
                })?;
                stream
                    .set_read_timeout(Some(PROXY_IO_TIMEOUT))
                    .map_err(|e| FetchError::HttpError {
                        url: addr.clone(),
                        reason: format!("set_read_timeout failed: {e}"),
                    })?;
                stream
                    .set_write_timeout(Some(PROXY_IO_TIMEOUT))
                    .map_err(|e| FetchError::HttpError {
                        url: addr.clone(),
                        reason: format!("set_write_timeout failed: {e}"),
                    })?;
                Ok(Box::new(stream))
            }
            #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
            ProxyEndpoint::Vsock { cid, port } => {
                let url = format!("vsock://{cid}:{port}");
                let stream =
                    vsock::VsockStream::connect_with_cid_port(*cid, *port).map_err(|e| {
                        FetchError::HttpError {
                            url: url.clone(),
                            reason: format!("vsock connect failed: {e}"),
                        }
                    })?;
                stream
                    .set_read_timeout(Some(PROXY_IO_TIMEOUT))
                    .map_err(|e| FetchError::HttpError {
                        url: url.clone(),
                        reason: format!("vsock set_read_timeout failed: {e}"),
                    })?;
                stream
                    .set_write_timeout(Some(PROXY_IO_TIMEOUT))
                    .map_err(|e| FetchError::HttpError {
                        url,
                        reason: format!("vsock set_write_timeout failed: {e}"),
                    })?;
                Ok(Box::new(stream))
            }
        }
    }
}

/// Sentinel value in `body_len` that signals chunked-stream framing — must
/// match `title_proxy::protocol::CHUNKED_SENTINEL`.
const CHUNKED_SENTINEL: u32 = u32::MAX;

/// End-of-stream marker meaning "the proxy hit MAX_RESPONSE_BYTES" — must
/// match `title_proxy::protocol::CHUNKED_TRUNCATED`. Treated as a fetch
/// failure on this side, not a complete body.
const CHUNKED_TRUNCATED: u32 = u32::MAX;

impl ContentFetcher for ProxyContentFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError> {
        let mut socket = self.open()?;

        // Request: [u32 method_len][method][u32 url_len][url][u32 body_len][body]
        write_string(&mut socket, "GET", url)?;
        write_string(&mut socket, url, url)?;
        write_bytes(&mut socket, &[], url)?;

        // Response: [u32 status][u32 body_len or CHUNKED_SENTINEL][body...]
        let status = read_u32(&mut socket, url)?;
        let body_len_field = read_u32(&mut socket, url)?;

        let body = if body_len_field == CHUNKED_SENTINEL {
            read_chunked_body(&mut *socket, self.max_body_bytes, url)?
        } else {
            let body_len = body_len_field as usize;
            if body_len > self.max_body_bytes {
                return Err(FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!(
                        "proxy body too large: {body_len} > max {}",
                        self.max_body_bytes
                    ),
                });
            }
            let mut buf = vec![0u8; body_len];
            socket
                .read_exact(&mut buf)
                .map_err(|e| FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!("body read failed after {body_len} bytes header: {e}"),
                })?;
            buf
        };

        // Status 0 is reserved for proxy-internal errors (DNS, TLS, etc.);
        // the body is a UTF-8 reason string in that case.
        if status == PROXY_INTERNAL_ERROR_STATUS {
            return Err(FetchError::HttpError {
                url: url.to_string(),
                reason: String::from_utf8_lossy(&body).into_owned(),
            });
        }

        if !(200..300).contains(&status) {
            return Err(FetchError::HttpStatus {
                status: status as u16,
                url: url.to_string(),
            });
        }

        if body.is_empty() {
            return Err(FetchError::EmptyContent(url.to_string()));
        }

        // The proxy doesn't forward Content-Type or ETag; rely on magic-byte
        // sniffing + URL extension. Range Requests aren't implemented yet
        // (Spec §5.2 future optimization), so ETag absence is harmless.
        let content_type = Some(detect_content_type(&body, url, None));

        Ok(FetchResponse {
            body,
            content_type,
            etag: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Wire helpers (sync — `ContentFetcher::fetch` is a blocking trait method,
// and the proxy crate's reference protocol uses synchronous `Read`/`Write`).
// ---------------------------------------------------------------------------

fn write_u32(w: &mut dyn Write, value: u32, url_for_err: &str) -> Result<(), FetchError> {
    w.write_all(&value.to_be_bytes())
        .map_err(|e| FetchError::HttpError {
            url: url_for_err.to_string(),
            reason: format!("proxy write_u32: {e}"),
        })
}

fn write_string(w: &mut dyn Write, value: &str, url_for_err: &str) -> Result<(), FetchError> {
    write_u32(w, value.len() as u32, url_for_err)?;
    w.write_all(value.as_bytes())
        .map_err(|e| FetchError::HttpError {
            url: url_for_err.to_string(),
            reason: format!("proxy write_string: {e}"),
        })
}

fn write_bytes(w: &mut dyn Write, value: &[u8], url_for_err: &str) -> Result<(), FetchError> {
    write_u32(w, value.len() as u32, url_for_err)?;
    w.write_all(value).map_err(|e| FetchError::HttpError {
        url: url_for_err.to_string(),
        reason: format!("proxy write_bytes: {e}"),
    })
}

fn read_u32(r: &mut dyn Read, url_for_err: &str) -> Result<u32, FetchError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).map_err(|e| FetchError::HttpError {
        url: url_for_err.to_string(),
        reason: format!("proxy read_u32: {e}"),
    })?;
    Ok(u32::from_be_bytes(buf))
}

/// Drain a chunked-body stream from the proxy. Each chunk is `[u32 len][bytes]`;
/// a zero-length chunk terminates the stream.
fn read_chunked_body(
    r: &mut dyn Read,
    max_body_bytes: usize,
    url_for_err: &str,
) -> Result<Vec<u8>, FetchError> {
    let mut body = Vec::new();
    loop {
        let n = read_u32(r, url_for_err)?;
        if n == 0 {
            return Ok(body);
        }
        if n == CHUNKED_TRUNCATED {
            return Err(FetchError::HttpError {
                url: url_for_err.to_string(),
                reason: format!(
                    "proxy truncated chunked response after {} bytes (upstream exceeded budget)",
                    body.len()
                ),
            });
        }
        let n = n as usize;
        if body.len().saturating_add(n) > max_body_bytes {
            return Err(FetchError::HttpError {
                url: url_for_err.to_string(),
                reason: format!(
                    "chunked body exceeded max {max_body_bytes} after {} bytes",
                    body.len()
                ),
            });
        }
        let start = body.len();
        body.resize(start + n, 0);
        r.read_exact(&mut body[start..])
            .map_err(|e| FetchError::HttpError {
                url: url_for_err.to_string(),
                reason: format!("chunked body read failed: {e}"),
            })?;
    }
}

// Helper trait so `open()` can return `Box<dyn ReadWrite>` regardless of
// whether the underlying socket is TCP or vsock.
trait ReadWrite: Read + Write + Send {}
impl<T: Read + Write + Send + ?Sized> ReadWrite for T {}

// ---------------------------------------------------------------------------
// Tests — exercise the TCP path end-to-end against the proxy crate's own
// length-prefixed protocol. The vsock path can't be exercised in unit tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    /// Tiny in-process stand-in for `title-proxy`. Reads one request,
    /// returns a canned response, then closes the connection.
    fn spawn_fake_proxy(
        response_status: u32,
        response_body: Vec<u8>,
    ) -> (
        String,
        thread::JoinHandle<Option<(String, String, Vec<u8>)>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().ok()?;
            // Read request header
            let method_len = read_u32_blocking(&mut stream)?;
            let method = read_n(&mut stream, method_len as usize)?;
            let url_len = read_u32_blocking(&mut stream)?;
            let url = read_n(&mut stream, url_len as usize)?;
            let body_len = read_u32_blocking(&mut stream)?;
            let body = read_n(&mut stream, body_len as usize)?;
            // Write canned response
            stream.write_all(&response_status.to_be_bytes()).ok()?;
            stream
                .write_all(&(response_body.len() as u32).to_be_bytes())
                .ok()?;
            stream.write_all(&response_body).ok()?;
            Some((
                String::from_utf8(method).ok()?,
                String::from_utf8(url).ok()?,
                body,
            ))
        });
        (addr, handle)
    }

    fn read_u32_blocking(r: &mut TcpStream) -> Option<u32> {
        let mut buf = [0u8; 4];
        r.read_exact(&mut buf).ok()?;
        Some(u32::from_be_bytes(buf))
    }

    fn read_n(r: &mut TcpStream, n: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; n];
        r.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    #[test]
    fn parse_tcp_endpoint() {
        match ProxyEndpoint::parse("127.0.0.1:8000").unwrap() {
            ProxyEndpoint::Tcp(s) => assert_eq!(s, "127.0.0.1:8000"),
            #[allow(unreachable_patterns)]
            _ => panic!("expected TCP"),
        }
    }

    #[test]
    fn fetch_success_round_trips_protocol() {
        let body = b"\xFF\xD8\xFFexample jpeg body".to_vec(); // JPEG magic
        let (addr, handle) = spawn_fake_proxy(200, body.clone());

        let fetcher = ProxyContentFetcher::new(ProxyEndpoint::Tcp(addr));
        let resp = fetcher.fetch("https://example.com/photo.jpg").unwrap();

        assert_eq!(resp.body, body);
        // Magic bytes were sniffed correctly.
        assert_eq!(resp.content_type.as_deref(), Some("image/jpeg"));
        assert!(resp.etag.is_none());

        let (method, url, request_body) = handle.join().unwrap().unwrap();
        assert_eq!(method, "GET");
        assert_eq!(url, "https://example.com/photo.jpg");
        assert!(request_body.is_empty());
    }

    #[test]
    fn fetch_non_2xx_returns_status_error() {
        let (addr, _h) = spawn_fake_proxy(404, b"not found".to_vec());
        let fetcher = ProxyContentFetcher::new(ProxyEndpoint::Tcp(addr));
        let err = fetcher.fetch("https://example.com/missing").unwrap_err();
        match err {
            FetchError::HttpStatus { status, .. } => assert_eq!(status, 404),
            other => panic!("expected HttpStatus, got {other:?}"),
        }
    }

    #[test]
    fn fetch_proxy_internal_error_surfaces_message() {
        let (addr, _h) = spawn_fake_proxy(0, b"DNS lookup failed".to_vec());
        let fetcher = ProxyContentFetcher::new(ProxyEndpoint::Tcp(addr));
        let err = fetcher.fetch("https://nowhere.invalid/").unwrap_err();
        match err {
            FetchError::HttpError { reason, .. } => assert!(reason.contains("DNS lookup failed")),
            other => panic!("expected HttpError, got {other:?}"),
        }
    }

    #[test]
    fn fetch_oversize_body_rejected_before_read() {
        let (addr, _h) = spawn_fake_proxy(200, vec![0u8; 1024]);
        let fetcher = ProxyContentFetcher::with_max_body_bytes(
            ProxyEndpoint::Tcp(addr),
            512, // smaller than the response
        );
        let err = fetcher.fetch("https://example.com/big").unwrap_err();
        match err {
            FetchError::HttpError { reason, .. } => assert!(reason.contains("too large")),
            other => panic!("expected HttpError, got {other:?}"),
        }
    }
}
