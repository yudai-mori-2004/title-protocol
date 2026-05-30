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

use title_core::{ContentSource, ContentStream};
use title_proxy::protocol as wire;

use crate::content_fetch::{
    detect_content_type, ContentFetcher, FetchError, FetchResponse, StreamingFetchResponse,
};

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

    /// 1 リクエスト分の socket を開く薄ラッパ。実体は free function
    /// [`open_socket`] にあり、`fetch` (本メソッド経由) と `ProxyRangeSource`
    /// (free function 直接呼び出し) で同じ実装を共有する。`url` はエラー
    /// メッセージのコンテキスト用 — 接続段階で失敗した場合にどの fetch かを
    /// ログから追える。
    fn open(&self, url: &str) -> Result<Box<dyn ReadWrite>, FetchError> {
        open_socket(&self.endpoint, url)
    }
}

// proxy crate からエクスポートされた定数を直接使う。以前は同じ値を tee 側で
// 別途定義していたが、wire 定数が分散すると silent regression を生むため、
// title-proxy の `protocol` モジュール一箇所を source-of-truth にする。
use wire::{CHUNKED_SENTINEL, CHUNKED_TRUNCATED, MAX_WIRE_CHUNK_BYTES};

impl ContentFetcher for ProxyContentFetcher {
    /// Spec §4.3 — proxy 経由の Range Request 対応。
    ///
    /// PROBE (= proxy は実装上 `GET Range: bytes=0-0`) を投げて 206 (= Range 対応)
    /// を確認し、対応していれば [`ProxyRangeSource`] を返す。non-206 (= 200 を含む)
    /// もしくは upstream エラーなら従来の full fetch にフォールバックする。
    /// HEAD ではなく Range GET を使う理由: R2 / S3 の SigV4 presigned GET URL は
    /// HTTP method が署名対象なので HEAD は SignatureDoesNotMatch で落ちるが、
    /// `Range` ヘッダは signed header に入っていないので Range GET なら署名が通る。
    fn fetch_streaming(&self, url: &str) -> Result<StreamingFetchResponse, FetchError> {
        match ProxyRangeSource::probe(self.endpoint.clone(), url, self.max_body_bytes) {
            Ok(source) => {
                let size_hint = source.size_hint();
                let etag = source.etag().map(|s| s.to_string());
                let content_type = source.content_type_hint().map(|s| s.to_string());
                Ok(StreamingFetchResponse {
                    source: Box::new(source),
                    content_type,
                    etag,
                    size_hint,
                })
            }
            Err(reason) => {
                // warn レベル。proxy 経由でも Range 非対応上流に当たった場合は
                // ピークメモリが file_size に膨らむ。運用視認性のため debug ではなく
                // warn にする (HttpContentFetcher 側と挙動を揃える)。
                tracing::warn!(
                    url,
                    %reason,
                    "proxy Range probe failed, falling back to full fetch (peak memory will be file size)"
                );
                let resp = self.fetch(url)?;
                if resp.body.is_empty() {
                    return Err(FetchError::EmptyContent(url.to_string()));
                }
                let body_len = resp.body.len();
                let content_type = resp.content_type.clone();
                let etag = resp.etag.clone();
                Ok(StreamingFetchResponse {
                    source: Box::new(title_core::InMemorySource::new(resp.body)),
                    content_type,
                    etag,
                    size_hint: Some(body_len as u64),
                })
            }
        }
    }

    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError> {
        let mut socket = self.open(url)?;

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

        // The proxy doesn't forward Content-Type or ETag on this `fetch` path
        // (full body). Magic-byte sniffing + URL extension に頼る。`fetch_streaming`
        // 経由の PROBE (`ProxyRangeSource::probe`) では Content-Type と ETag
        // が wire-encoded で返るため、Range Request 経路は ETag 一致を維持できる。
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
        if n > MAX_WIRE_CHUNK_BYTES {
            return Err(FetchError::HttpError {
                url: url_for_err.to_string(),
                reason: format!(
                    "chunk_len {n} exceeds wire limit {MAX_WIRE_CHUNK_BYTES} — likely proxy fault",
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
// Proxy-mediated Range Request (Spec §4.3)
// ---------------------------------------------------------------------------

/// 共有可能な proxy 接続パラメータ。`HttpReader` 相当の reader を構築する
/// たびに新しい socket を開くため、endpoint と max_body_bytes を `Arc` で
/// 共有する。
#[derive(Clone)]
struct ProxyConn {
    endpoint: ProxyEndpoint,
    max_body_bytes: usize,
}

/// `http_range_client::SyncHttpRangeClient` を proxy ワイヤープロトコル経由で
/// 実装したアダプタ。PROBE は `wire::encode_probe_response` で構造化された
/// 応答を proxy が返す前提。Range GET は `GET_RANGE` 専用メソッドを使い、
/// `[u64 begin][u64 length]` の固定 16 byte body を送る。
///
/// `http_range_client::SyncBufferedHttpRangeClient` に被せることで、buffering
/// と Read+Seek 実装をそのまま再利用できる。
struct ProxyRangeBackend {
    conn: ProxyConn,
}

impl http_range_client::SyncHttpRangeClient for ProxyRangeBackend {
    fn get_range(
        &self,
        url: &str,
        range: &str,
    ) -> http_range_client::Result<bytes::Bytes> {
        // range 文字列は "bytes=BEGIN-END" の形 (RFC 7233)。
        let stripped = range.strip_prefix("bytes=").ok_or_else(|| {
            http_range_client::HttpError::HttpError(format!("invalid range: {range}"))
        })?;
        let (begin_s, end_s) = stripped.split_once('-').ok_or_else(|| {
            http_range_client::HttpError::HttpError(format!("invalid range: {range}"))
        })?;
        let begin: u64 = begin_s.parse().map_err(|_| {
            http_range_client::HttpError::HttpError(format!("invalid range begin: {range}"))
        })?;
        let end: u64 = end_s.parse().map_err(|_| {
            http_range_client::HttpError::HttpError(format!("invalid range end: {range}"))
        })?;
        let length = end.saturating_sub(begin).saturating_add(1);

        let body = wire::encode_get_range_body(begin, length);
        let (status, response_body) = send_wire_request(
            &self.conn.endpoint,
            "GET_RANGE",
            url,
            &body,
            self.conn.max_body_bytes,
        )
        .map_err(|e| http_range_client::HttpError::HttpError(format!("proxy GET_RANGE: {e}")))?;

        if status == PROXY_INTERNAL_ERROR_STATUS {
            return Err(http_range_client::HttpError::HttpError(format!(
                "proxy internal error: {}",
                String::from_utf8_lossy(&response_body)
            )));
        }

        if !(200..300).contains(&status) {
            return Err(http_range_client::HttpError::HttpStatus(status as u16));
        }

        Ok(bytes::Bytes::from(response_body))
    }

    fn head_response_header(
        &self,
        url: &str,
        header: &str,
    ) -> http_range_client::Result<Option<String>> {
        let (status, body) = send_wire_request(
            &self.conn.endpoint,
            "PROBE",
            url,
            &[],
            self.conn.max_body_bytes,
        )
        .map_err(|e| http_range_client::HttpError::HttpError(format!("proxy PROBE: {e}")))?;

        if status == PROXY_INTERNAL_ERROR_STATUS {
            return Err(http_range_client::HttpError::HttpError(format!(
                "proxy internal error: {}",
                String::from_utf8_lossy(&body)
            )));
        }
        if !(200..300).contains(&status) {
            return Err(http_range_client::HttpError::HttpStatus(status as u16));
        }

        let parsed = wire::decode_probe_response(&body)
            .map_err(|e| http_range_client::HttpError::HttpError(e.to_string()))?;

        let value = match header.to_ascii_lowercase().as_str() {
            "content-length" => Some(parsed.content_length.to_string()),
            "accept-ranges" => Some(if parsed.accepts_ranges { "bytes" } else { "none" }.to_string()),
            "etag" => parsed.etag,
            "content-type" => parsed.content_type,
            _ => None,
        };
        Ok(value)
    }
}

/// proxy 経由で 1 リクエスト送って応答 (single-frame) を読み取る低レイヤ helper。
/// `body_len_field == CHUNKED_SENTINEL` 経路は PROBE/GET_RANGE では使われない想定。
fn send_wire_request(
    endpoint: &ProxyEndpoint,
    method: &str,
    url: &str,
    body: &[u8],
    max_body_bytes: usize,
) -> Result<(u32, Vec<u8>), FetchError> {
    let mut socket = open_socket(endpoint, url)?;

    write_string(&mut socket, method, url)?;
    write_string(&mut socket, url, url)?;
    write_bytes(&mut socket, body, url)?;

    let status = read_u32(&mut socket, url)?;
    let body_len_field = read_u32(&mut socket, url)?;
    if body_len_field == CHUNKED_SENTINEL {
        return Err(FetchError::HttpError {
            url: url.to_string(),
            reason: "Unexpected chunked framing on PROBE/GET_RANGE response".to_string(),
        });
    }
    let body_len = body_len_field as usize;
    if body_len > max_body_bytes {
        return Err(FetchError::HttpError {
            url: url.to_string(),
            reason: format!("proxy body too large: {body_len} > max {max_body_bytes}"),
        });
    }
    let mut buf = vec![0u8; body_len];
    socket
        .read_exact(&mut buf)
        .map_err(|e| FetchError::HttpError {
            url: url.to_string(),
            reason: format!("body read failed: {e}"),
        })?;

    Ok((status, buf))
}

/// `ProxyContentFetcher::open` 相当の薄いヘルパ (free function 版)。
fn open_socket(endpoint: &ProxyEndpoint, url: &str) -> Result<Box<dyn ReadWrite>, FetchError> {
    match endpoint {
        ProxyEndpoint::Tcp(addr) => {
            let stream = TcpStream::connect_timeout(
                &addr.parse().map_err(|e| FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!("invalid proxy addr: {e}"),
                })?,
                PROXY_CONNECT_TIMEOUT,
            )
            .map_err(|e| FetchError::HttpError {
                url: url.to_string(),
                reason: format!("proxy connect failed: {e}"),
            })?;
            stream
                .set_read_timeout(Some(PROXY_IO_TIMEOUT))
                .map_err(|e| FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!("set_read_timeout failed: {e}"),
                })?;
            stream
                .set_write_timeout(Some(PROXY_IO_TIMEOUT))
                .map_err(|e| FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!("set_write_timeout failed: {e}"),
                })?;
            Ok(Box::new(stream))
        }
        #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
        ProxyEndpoint::Vsock { cid, port } => {
            let url_label = format!("vsock://{cid}:{port}");
            let stream = vsock::VsockStream::connect_with_cid_port(*cid, *port).map_err(|e| {
                FetchError::HttpError {
                    url: url_label.clone(),
                    reason: format!("vsock connect failed: {e}"),
                }
            })?;
            stream
                .set_read_timeout(Some(PROXY_IO_TIMEOUT))
                .map_err(|e| FetchError::HttpError {
                    url: url_label.clone(),
                    reason: format!("vsock set_read_timeout failed: {e}"),
                })?;
            stream
                .set_write_timeout(Some(PROXY_IO_TIMEOUT))
                .map_err(|e| FetchError::HttpError {
                    url: url_label,
                    reason: format!("vsock set_write_timeout failed: {e}"),
                })?;
            Ok(Box::new(stream))
        }
    }
}

/// Proxy 経由 Range Request 用 `ContentSource`。
///
/// `probe` で HEAD を打ち、Range 対応とサイズを確認してから construct する。
/// `open()` は `SyncBufferedHttpRangeClient<ProxyRangeBackend>` を返し、
/// http-range-client のバッファリングと Read+Seek 実装を借用する。
pub struct ProxyRangeSource {
    conn: ProxyConn,
    url: String,
    content_length: u64,
    etag: Option<String>,
    content_type_hint: Option<String>,
    min_req_size: usize,
}

impl ProxyRangeSource {
    /// デフォルトの最小リクエストサイズ。
    /// [`crate::range_source::HttpRangeSource::DEFAULT_MIN_REQ_SIZE`] と同じ
    /// 値を共有する (4 MiB)。理由はそちらの doc を参照。
    pub const DEFAULT_MIN_REQ_SIZE: usize = crate::range_source::HttpRangeSource::DEFAULT_MIN_REQ_SIZE;

    /// PROBE (proxy が内部で `GET Range: bytes=0-0` を発行) で Range 対応 +
    /// Content-Length (= 206 時の Content-Range の分母) を確認する。
    pub fn probe(
        endpoint: ProxyEndpoint,
        url: &str,
        max_body_bytes: usize,
    ) -> Result<Self, FetchError> {
        let (status, probe_body) =
            send_wire_request(&endpoint, "PROBE", url, &[], max_body_bytes)?;

        if status == PROXY_INTERNAL_ERROR_STATUS {
            return Err(FetchError::HttpError {
                url: url.to_string(),
                reason: format!(
                    "proxy PROBE returned internal error: {}",
                    String::from_utf8_lossy(&probe_body)
                ),
            });
        }
        if !(200..300).contains(&status) {
            return Err(FetchError::HttpStatus {
                status: status as u16,
                url: url.to_string(),
            });
        }

        let parsed = wire::decode_probe_response(&probe_body).map_err(|e| FetchError::HttpError {
            url: url.to_string(),
            reason: format!("PROBE response decode failed: {e}"),
        })?;

        if !parsed.accepts_ranges {
            return Err(FetchError::HttpError {
                url: url.to_string(),
                reason: "Upstream did not return 206 to Range probe (= no Range support)".to_string(),
            });
        }

        if parsed.content_length == 0 {
            return Err(FetchError::EmptyContent(url.to_string()));
        }

        Ok(Self {
            conn: ProxyConn {
                endpoint,
                max_body_bytes,
            },
            url: url.to_string(),
            content_length: parsed.content_length,
            etag: parsed.etag,
            content_type_hint: parsed.content_type,
            min_req_size: Self::DEFAULT_MIN_REQ_SIZE,
        })
    }

    pub fn with_min_req_size(mut self, size: usize) -> Self {
        self.min_req_size = size;
        self
    }

    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    pub fn content_type_hint(&self) -> Option<&str> {
        self.content_type_hint.as_deref()
    }
}

impl ContentSource for ProxyRangeSource {
    fn open(&self) -> std::io::Result<Box<dyn ContentStream>> {
        let backend = ProxyRangeBackend {
            conn: self.conn.clone(),
        };
        let mut reader = http_range_client::SyncBufferedHttpRangeClient::with(backend, &self.url);
        reader.set_min_req_size(self.min_req_size);
        // SafeRangeReader (上流の Read::read バグ + EOF 規約違反 + 末尾跨ぎ
        // バッファロストを吸収する共通アダプタ、`crates/tee/src/range_source.rs`
        // に定義) で包む。HttpRangeSource と同じ実装を再利用するため、direct HTTP と
        // proxy で実装重複が消える。PROBE で確認済みの `content_length` を渡し、
        // EOF を超えるリクエストを未然に防ぐ。
        Ok(Box::new(crate::range_source::SafeRangeReader::new(
            reader,
            self.content_length,
        )))
    }

    fn size_hint(&self) -> Option<u64> {
        Some(self.content_length)
    }

    /// 仕様 §4.3 — proxy 経由 Range Request のピークメモリ見積もり。
    /// [`crate::range_source::HttpRangeSource::peak_memory_hint`] と同じ理屈で
    /// `min_req_size` (デフォルト 4 MiB) を申告する。c2pa-rs の単発 read を
    /// `min_req_size` 内に収める前提で、ResourcePool admission control が
    /// 正しく機能する。
    fn peak_memory_hint(&self) -> Option<u64> {
        Some(self.min_req_size as u64)
    }

    /// proxy 経由 Range Request も常駐ゼロ。`HttpRangeSource` と同じ理屈で
    /// `false`。reader 内部の vsock/TCP 接続バッファのみが active 時の RAM。
    fn is_in_memory_resident(&self) -> bool {
        false
    }
}


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

    // ---- Range Request 経由 (Spec §4.3) ----

    /// 複数リクエスト (HEAD + GET_RANGE × N) を受ける fake proxy。
    /// 1 接続 = 1 リクエストの wire protocol を満たすため、connection ごとに新 thread。
    fn spawn_multi_request_proxy(
        upstream_body: Vec<u8>,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || loop {
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => break,
            };
            let body = upstream_body.clone();
            thread::spawn(move || handle_one_request(stream, body));
        });
        (addr, handle)
    }

    fn handle_one_request(mut stream: TcpStream, body: Vec<u8>) {
        let method = match read_string(&mut stream) {
            Ok(m) => m,
            Err(_) => return,
        };
        let _url = match read_string(&mut stream) {
            Ok(u) => u,
            Err(_) => return,
        };
        let req_body = match read_bytes(&mut stream) {
            Ok(b) => b,
            Err(_) => return,
        };

        match method.as_str() {
            "PROBE" => {
                let probe_body = wire::encode_probe_response(
                    body.len() as u64,
                    true,
                    Some("\"proxy-test\""),
                    Some("video/mp4"),
                );
                let _ = stream.write_all(&200u32.to_be_bytes());
                let _ = stream.write_all(&(probe_body.len() as u32).to_be_bytes());
                let _ = stream.write_all(&probe_body);
            }
            "GET_RANGE" => {
                let (begin, length) =
                    wire::decode_get_range_body(&req_body).expect("valid range body");
                let begin = begin as usize;
                let length = length as usize;
                let end = (begin + length).min(body.len());
                if begin >= body.len() {
                    // EOF → 416
                    let _ = stream.write_all(&416u32.to_be_bytes());
                    let _ = stream.write_all(&0u32.to_be_bytes());
                } else {
                    let slice = &body[begin..end];
                    let _ = stream.write_all(&206u32.to_be_bytes());
                    let _ = stream.write_all(&(slice.len() as u32).to_be_bytes());
                    let _ = stream.write_all(slice);
                }
            }
            _ => {
                let _ = stream.write_all(&0u32.to_be_bytes());
                let msg = b"Unsupported method";
                let _ = stream.write_all(&(msg.len() as u32).to_be_bytes());
                let _ = stream.write_all(msg);
            }
        }
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }

    fn read_string(stream: &mut TcpStream) -> std::io::Result<String> {
        let buf = read_bytes(stream)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn read_bytes(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf)?;
        Ok(buf)
    }

    #[test]
    fn proxy_range_source_probe_succeeds() {
        let body: Vec<u8> = (0..1024).map(|i| (i % 251) as u8).collect();
        let (addr, _h) = spawn_multi_request_proxy(body);
        let source = ProxyRangeSource::probe(
            ProxyEndpoint::Tcp(addr),
            "https://example.com/file.mp4",
            ProxyContentFetcher::DEFAULT_MAX_BODY_BYTES,
        )
        .expect("probe");
        assert_eq!(source.size_hint(), Some(1024));
        assert_eq!(source.etag(), Some("\"proxy-test\""));
        assert_eq!(source.content_type_hint(), Some("video/mp4"));
    }

    #[test]
    fn proxy_range_source_read_returns_correct_bytes() {
        let body: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let (addr, _h) = spawn_multi_request_proxy(body.clone());

        let source = ProxyRangeSource::probe(
            ProxyEndpoint::Tcp(addr),
            "https://example.com/file.mp4",
            ProxyContentFetcher::DEFAULT_MAX_BODY_BYTES,
        )
        .expect("probe")
        .with_min_req_size(256);

        let mut reader = source.open().expect("open");
        let mut got = Vec::new();
        reader.read_to_end(&mut got).expect("read_to_end");
        assert_eq!(got, body);
    }

    #[test]
    fn proxy_range_source_seek_then_read() {
        use std::io::SeekFrom;
        let body: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let (addr, _h) = spawn_multi_request_proxy(body.clone());

        let source = ProxyRangeSource::probe(
            ProxyEndpoint::Tcp(addr),
            "https://example.com/file.mp4",
            ProxyContentFetcher::DEFAULT_MAX_BODY_BYTES,
        )
        .expect("probe")
        .with_min_req_size(128);

        let mut reader = source.open().expect("open");
        reader.seek(SeekFrom::Start(1500)).unwrap();
        let mut buf = vec![0u8; 200];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, body[1500..1700]);
    }

    // ---- ContentSource contract suite (proxy Range backend) ----
    //
    // HttpRangeSource と同じ contract を proxy 経由でも回す。Read::read 規約
    // (返却 N に対し buf[..N] のみ valid) を境界条件込みで構造的に保証する。

    fn run_proxy_contract(body_len: usize, min_req_size: usize) {
        let body: Vec<u8> = (0..body_len).map(|i| (i % 251) as u8).collect();
        let (addr, _h) = spawn_multi_request_proxy(body.clone());
        let source = ProxyRangeSource::probe(
            ProxyEndpoint::Tcp(addr),
            "https://example.com/contract.bin",
            ProxyContentFetcher::DEFAULT_MAX_BODY_BYTES,
        )
        .expect("probe")
        .with_min_req_size(min_req_size);
        title_core::content_stream::contract::assert_content_source_contract(&source, &body);
    }

    /// 末尾跨ぎ (file_size % min_req_size != 0) 境界
    #[test]
    fn proxy_range_source_contract_tail_not_aligned() {
        run_proxy_contract(1023, 256);
        run_proxy_contract(1024 + 1, 256);
        run_proxy_contract(255, 256);
        run_proxy_contract(257, 256);
    }

    #[test]
    fn proxy_range_source_contract_aligned_sizes() {
        run_proxy_contract(1024, 256);
        run_proxy_contract(4096, 512);
    }

    #[test]
    fn proxy_range_source_contract_min_req_larger_than_body() {
        run_proxy_contract(500, 64 * 1024);
    }

    #[test]
    fn proxy_fetcher_fetch_streaming_uses_range_source() {
        let body: Vec<u8> = (0..2048).map(|i| (i % 251) as u8).collect();
        let (addr, _h) = spawn_multi_request_proxy(body.clone());

        let fetcher = ProxyContentFetcher::new(ProxyEndpoint::Tcp(addr));
        let resp = fetcher
            .fetch_streaming("https://example.com/file.mp4")
            .expect("fetch_streaming");
        assert_eq!(resp.size_hint, Some(2048));
        assert_eq!(resp.content_type.as_deref(), Some("video/mp4"));

        let mut reader = resp.source.open().unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, body);
    }
}
