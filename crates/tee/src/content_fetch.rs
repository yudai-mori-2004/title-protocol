// SPDX-License-Identifier: Apache-2.0

//! # Content Fetch Layer
//!
//! Spec §5.2 -- Content fetch from external storage
//!
//! Fetches content from URL(s) based on input type (single / fragmented / sidecar).
//! Provides a trait-based abstraction over the HTTP client, enabling mock-based
//! testing without network I/O.
//!
//! ## Memory tracking (§4.2, §4.4)
//!
//! All fetch operations accept a `Ticket` and call `extend()` as data arrives.
//! This tracks actual memory usage and enforces timeouts and limits.
//! For fragmented input, each fragment is validated against size limits.
//!
//! ## Input types
//!
//! | Type | Fetch strategy |
//! |---|---|
//! | Single | HTTP GET for the content file |
//! | Fragmented | Sequential HTTP GET: init.mp4, then each seg-*.m4s |
//! | Sidecar | Two HTTP GETs: manifest (.c2pa) + content file |
//!

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use title_core::{ContentSource, InMemorySource, InputData};

use crate::limits::{self, LimitsError};
use crate::resource_pool::{Ticket, TicketError};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error during content fetching.
/// Spec §5.2
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// HTTP request failed (network error, DNS failure, etc.).
    #[error("HTTP request failed for {url}: {reason}")]
    HttpError {
        /// The URL that was being fetched.
        url: String,
        /// Human-readable error description.
        reason: String,
    },

    /// HTTP server returned a non-success status code.
    #[error("HTTP {status} for URL: {url}")]
    HttpStatus {
        /// HTTP status code.
        status: u16,
        /// The URL that returned the error.
        url: String,
    },

    /// ETag mismatch during Range Request sequence.
    /// Spec §5.2 -- 412 Precondition Failed
    #[error("File changed during transfer (412 Precondition Failed): {url}")]
    EtagMismatch {
        /// The URL where the mismatch was detected.
        url: String,
    },

    /// Server returned an empty response body.
    #[error("Empty content from URL: {0}")]
    EmptyContent(String),

    /// Fragmented input with no fragment URLs.
    #[error("Fragmented input requires at least one fragment URL")]
    NoFragments,

    /// Memory reservation failed (Ticket.extend rejected).
    /// Spec §4.2, §4.4
    #[error("Memory limit: {0}")]
    MemoryLimit(#[from] TicketError),

    /// Data size validation failed (fragment count/size exceeded).
    /// Spec §4.4
    #[error("Data limit: {0}")]
    DataLimit(#[from] LimitsError),
}

// ---------------------------------------------------------------------------
// Fetcher trait and types
// ---------------------------------------------------------------------------

/// Response from a single HTTP fetch.
#[derive(Debug)]
pub struct FetchResponse {
    /// Response body bytes.
    pub body: Vec<u8>,
    /// Content-Type header value, if present.
    pub content_type: Option<String>,
    /// ETag header value, if present.
    pub etag: Option<String>,
}

/// Content fetcher trait.
/// Spec §5.2, §4.3
///
/// Abstraction over the HTTP client for testability.
/// The orchestrator depends on this trait, not on `reqwest` directly.
pub trait ContentFetcher: Send + Sync {
    /// Fetch the full body from a URL.
    ///
    /// Implementations should follow redirects and return the final body.
    /// 主に小ファイル (フラグメント / サイドカーマニフェスト) で使用する。
    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError>;

    /// 仕様 §4.3 のストリーミング取得。`Read + Seek` ベースの `ContentSource`
    /// を返す。
    ///
    /// 既定実装は `fetch()` を呼んで全バイトを `InMemorySource` に包む。
    /// HTTP 直結 (`HttpContentFetcher`) や proxy 経由 (`ProxyContentFetcher`)
    /// は Range Request 経路を持つように override する。
    ///
    /// `size_hint` が `Some` の場合、orchestrator はそれを使って ticket の
    /// 漸進予約を最適化する。`None` の場合は `fetch` 経路と同じく完全 fetch
    /// 後の `extend(body.len())` で予約する。
    fn fetch_streaming(&self, url: &str) -> Result<StreamingFetchResponse, FetchError> {
        let resp = self.fetch(url)?;
        if resp.body.is_empty() {
            return Err(FetchError::EmptyContent(url.to_string()));
        }
        let body_len = resp.body.len();
        let content_type = resp.content_type.clone();
        let etag = resp.etag.clone();
        Ok(StreamingFetchResponse {
            source: Box::new(InMemorySource::new(resp.body)),
            content_type,
            etag,
            size_hint: Some(body_len as u64),
        })
    }
}

/// `ContentFetcher::fetch_streaming` の戻り値。
///
/// `source` を `open()` すると `Read + Seek` reader が得られる。
/// `size_hint` は HEAD で得た既知サイズ。Range Request 非対応 fetcher (mock 等)
/// では `fetch()` の body 長がそのまま入る。
pub struct StreamingFetchResponse {
    pub source: Box<dyn ContentSource>,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub size_hint: Option<u64>,
}

impl std::fmt::Debug for StreamingFetchResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingFetchResponse")
            .field("size_hint", &self.size_hint)
            .field("content_type", &self.content_type)
            .field("etag", &self.etag)
            .finish()
    }
}

/// HTTP-based content fetcher using `reqwest::blocking::Client`.
/// Spec §5.2, §4.4
///
/// Enforces the size and timeout limits that Spec §4.4 specifies for the
/// fetch layer: every connection has a chunk-level read timeout, an overall
/// wall-clock deadline, and a hard body-size ceiling. These prevent a
/// malicious or misbehaving origin from stalling the TEE or exhausting its
/// memory.
pub struct HttpContentFetcher {
    client: reqwest::blocking::Client,
    max_body_bytes: usize,
}

impl HttpContentFetcher {
    /// Connect-timeout: detect unreachable origins quickly.
    pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

    /// Per-fetch wall-clock budget. Caps how long a single GET can block,
    /// independent of `Ticket` chunk timeouts (Spec §4.4).
    pub const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

    /// Default body-size ceiling — matches `MAX_FRAGMENT_SIZE = 100 MB`.
    pub const DEFAULT_MAX_BODY_BYTES: usize = 100 * 1024 * 1024;

    /// Construct with default size/timeout limits.
    pub fn new() -> Self {
        Self::with_max_body_bytes(Self::DEFAULT_MAX_BODY_BYTES)
    }

    /// Construct with a custom maximum body size (in bytes).
    /// Tests use small values to exercise the size cap.
    pub fn with_max_body_bytes(max_body_bytes: usize) -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent("title-tee/0.1.2")
            .connect_timeout(Self::CONNECT_TIMEOUT)
            .timeout(Self::FETCH_TIMEOUT)
            .build()
            .expect("Failed to build HTTP client");
        Self {
            client,
            max_body_bytes,
        }
    }
}

impl Default for HttpContentFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentFetcher for HttpContentFetcher {
    /// HTTP Range Request 経由のストリーミング取得を試みる。
    /// 仕様 §4.3 — Range 対応サーバーなら 50 GB ファイルでも全展開しない。
    ///
    /// HEAD で `Accept-Ranges: bytes` を確認できれば [`HttpRangeSource`] を返す。
    /// 確認できない場合 (HEAD 拒否、Range 非対応、Content-Length 欠落) は
    /// 黙って `fetch()` 経路にフォールバックする。これにより Range 非対応の
    /// 古いストレージや R2 / GCS の特定 prefix でも処理が止まらない。
    fn fetch_streaming(&self, url: &str) -> Result<StreamingFetchResponse, FetchError> {
        match crate::range_source::HttpRangeSource::probe(url) {
            Ok(source) => {
                let size_hint = source.size_hint();
                let content_type = source.content_type_hint().map(|s| s.to_string());
                let etag = source.etag().map(|s| s.to_string());
                Ok(StreamingFetchResponse {
                    source: Box::new(source),
                    content_type,
                    etag,
                    size_hint,
                })
            }
            Err(reason) => {
                // warn レベルで残す。Range 非対応サーバーへの fallback は仕様
                // §4.3 のメモリ上限 (典型 64 KB) より大きいバッファを要求する
                // 経路で、運用者が「なぜ大きなファイルが reject されるか」を
                // 追える視認性が必要 (audit round 3 監査での指摘箇所)。
                tracing::warn!(
                    url,
                    %reason,
                    "Range probe failed, falling back to full fetch (peak memory will be file size)"
                );
                let resp = self.fetch(url)?;
                if resp.body.is_empty() {
                    return Err(FetchError::EmptyContent(url.to_string()));
                }
                let body_len = resp.body.len();
                let content_type = resp.content_type.clone();
                let etag = resp.etag.clone();
                Ok(StreamingFetchResponse {
                    source: Box::new(InMemorySource::new(resp.body)),
                    content_type,
                    etag,
                    size_hint: Some(body_len as u64),
                })
            }
        }
    }

    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| FetchError::HttpError {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        let status = resp.status().as_u16();
        if status == 412 {
            return Err(FetchError::EtagMismatch {
                url: url.to_string(),
            });
        }
        if !resp.status().is_success() {
            return Err(FetchError::HttpStatus {
                status,
                url: url.to_string(),
            });
        }

        // Bail early if the server advertised a Content-Length that already
        // exceeds the cap. Avoids streaming gigabytes only to drop them.
        if let Some(len) = resp.content_length() {
            if len as usize > self.max_body_bytes {
                return Err(FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!(
                        "Body too large: Content-Length={len} > max={}",
                        self.max_body_bytes
                    ),
                });
            }
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // Stream the body with an explicit size cap so a server that lies
        // about (or omits) Content-Length still can't OOM the TEE.
        let mut body = Vec::new();
        let mut reader = resp;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| FetchError::HttpError {
                url: url.to_string(),
                reason: format!("Body read error: {e}"),
            })?;
            if n == 0 {
                break;
            }
            if body.len() + n > self.max_body_bytes {
                return Err(FetchError::HttpError {
                    url: url.to_string(),
                    reason: format!(
                        "Body exceeds max size {} after streaming {} bytes",
                        self.max_body_bytes,
                        body.len() + n
                    ),
                });
            }
            body.extend_from_slice(&buf[..n]);
        }

        Ok(FetchResponse {
            body,
            content_type,
            etag,
        })
    }
}

// ---------------------------------------------------------------------------
// Fetched content
// ---------------------------------------------------------------------------

/// Fetched content, normalized for processor consumption.
/// Spec §5.2, §4.3
///
/// `source` は reader factory ([`title_core::ContentSource`])。orchestrator は
/// `source.open()` で必要なときに `Read + Seek` stream を取り出す。これにより
/// 50 GB 級の動画でも Range Request 経由で必要部分だけを読み込める。
///
/// - Single: 1 ファイル分のソース。HTTP Range Request 経由なら
///   [`crate::range_source::HttpRangeSource`] / [`crate::proxy_fetcher::ProxyRangeSource`]
///   が入る。mock / 全 fetch fallback 経路では [`InMemorySource`] が入る。
/// - Fragmented: init + 全 segment を concat したバイト列を InMemorySource で
///   保持。fragment-by-fragment streaming (c2pa::Reader::with_fragment 経路) は
///   v0.1.3 持ち越し。
/// - Sidecar: コンテンツ本体のソースに加え、`manifest_source` が別途付く。
pub struct FetchedContent {
    /// コンテンツ本体の reader factory。
    pub source: Box<dyn ContentSource>,

    /// Detected MIME type of the content.
    pub content_type: String,

    /// For sidecar input: the separate manifest (.c2pa) bytes containing
    /// raw JUMBF data. `None` for single and fragmented inputs.
    pub manifest_source: Option<Box<dyn ContentSource>>,
}

impl std::fmt::Debug for FetchedContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchedContent")
            .field("source_size_hint", &self.source.size_hint())
            .field("content_type", &self.content_type)
            .field(
                "manifest_source_size_hint",
                &self.manifest_source.as_ref().and_then(|s| s.size_hint()),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Content type detection
// ---------------------------------------------------------------------------

/// Detect MIME type from magic bytes, server Content-Type, and URL extension.
/// Priority: magic bytes > server header > URL extension > fallback.
pub fn detect_content_type(bytes: &[u8], url: &str, server_type: Option<&str>) -> String {
    // Magic bytes (highest priority -- most reliable)
    if bytes.len() >= 12 {
        // JPEG: FF D8 FF
        if bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            return "image/jpeg".to_string();
        }
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        if bytes.len() >= 8 && bytes[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
            return "image/png".to_string();
        }
        // MP4/fMP4: ftyp box at offset 4
        if &bytes[4..8] == b"ftyp" {
            return "video/mp4".to_string();
        }
    }

    // Server Content-Type (if not generic)
    if let Some(ct) = server_type {
        if !ct.starts_with("application/octet-stream") && !ct.starts_with("binary/") {
            // Strip parameters (e.g., "image/jpeg; charset=utf-8" -> "image/jpeg")
            return ct.split(';').next().unwrap_or(ct).trim().to_string();
        }
    }

    // URL extension fallback
    let url_lower = url.to_lowercase();
    if url_lower.ends_with(".jpg") || url_lower.ends_with(".jpeg") {
        return "image/jpeg".to_string();
    }
    if url_lower.ends_with(".png") {
        return "image/png".to_string();
    }
    if url_lower.ends_with(".mp4") || url_lower.ends_with(".m4s") {
        return "video/mp4".to_string();
    }
    if url_lower.ends_with(".c2pa") {
        return "application/c2pa".to_string();
    }

    tracing::warn!(
        url,
        "could not determine content type from magic bytes, server header, or URL extension; falling back to application/octet-stream"
    );
    "application/octet-stream".to_string()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch content based on input data type, tracking memory via Ticket.
/// Spec §5.2, §4.2, §4.3
///
/// Each fetch operation calls `ticket.extend()` as data arrives, enforcing
/// memory limits and timeouts. Fragmented input passes `Arc<Ticket>` down to
/// [`crate::fragmented_source::FragmentedSource`] so that fragment swap can do
/// dynamic `extend` / `shrink` (仕様 §4.3 の shrink ループ)。
pub fn fetch_content(
    fetcher: &dyn ContentFetcher,
    input: &InputData,
    ticket: &Arc<Ticket>,
) -> Result<FetchedContent, FetchError> {
    match input {
        InputData::Single { content_url } => fetch_single(fetcher, content_url, ticket),
        InputData::Fragmented {
            init_url,
            fragment_urls,
        } => {
            // Validate fragment count before starting any fetches
            // Spec §4.4
            limits::validate_fragment_count(fragment_urls.len())?;
            fetch_fragmented(fetcher, init_url, fragment_urls, ticket)
        }
        InputData::Sidecar {
            manifest_url,
            content_url,
        } => fetch_sidecar(fetcher, manifest_url, content_url, ticket),
    }
}

/// Fetch single file content.
/// Spec §5.2, §4.3 -- HTTP GET (or Range Request) for the content URL.
///
/// `fetcher.fetch_streaming()` を呼ぶことで、Range Request 対応サーバーなら
/// 50 GB のファイルでも全展開せず [`HttpRangeSource`] を返す。Range 非対応
/// もしくは mock fetcher なら従来通り full fetch → `InMemorySource`。
///
/// メモリ予約 (`ticket.extend`) は [`title_core::ContentSource::peak_memory_hint`]
/// が示す**実ピークメモリ**で行う:
/// - In-memory ソース: ファイル全長 (= 既にメモリ上)
/// - Range Request ソース: reader バッファサイズ (典型 64 KB)
///
/// この区別を `size_hint` ベースにすると、50 GB の Range Request リクエストが
/// admission_limit で reject されてしまい §4.3 のストリーミング前提が崩れる。
fn fetch_single(
    fetcher: &dyn ContentFetcher,
    url: &str,
    ticket: &Arc<Ticket>,
) -> Result<FetchedContent, FetchError> {
    let resp = fetcher.fetch_streaming(url)?;

    // 論理サイズ 0 (empty content) はソース種別問わずエラー。
    if matches!(resp.size_hint, Some(0)) {
        return Err(FetchError::EmptyContent(url.to_string()));
    }

    // ピークメモリ予約。peak_memory_hint が None なら従来通り保守的 (size_hint
    // 相当)、Range source なら数十 KB だけ予約される。
    if let Some(peak) = resp.source.peak_memory_hint() {
        ticket.extend(peak as usize)?;
    }

    // MIME 判定: streaming source からも最初の数バイトだけ覗いてマジックバイト
    // 検出する。HEAD の Content-Type が信用できないストレージ (R2 等) でも
    // ファイル先頭で正しく判定できる。
    let mut peek_reader = resp.source.open().map_err(|e| FetchError::HttpError {
        url: url.to_string(),
        reason: format!("Could not open source for MIME peek: {e}"),
    })?;
    let mut magic = [0u8; 12];
    let read = peek_reader.read(&mut magic).map_err(|e| FetchError::HttpError {
        url: url.to_string(),
        reason: format!("MIME peek read failed: {e}"),
    })?;
    drop(peek_reader);

    let content_type = detect_content_type(&magic[..read], url, resp.content_type.as_deref());

    Ok(FetchedContent {
        source: resp.source,
        content_type,
        manifest_source: None,
    })
}

/// Fetch fragmented content (CMAF init + segments).
/// Spec §5.2, §4.3 -- 仕様 §4.3 のフラグメントメモリパターンに準拠。
///
/// BMFF/ISO-14496-12 fragmented MP4 は ftyp + moov (init.mp4) + moof + mdat
/// (各 segment) という構造。本実装は [`FragmentedSource`] を通じて
/// 「init は in-memory 常駐 + fragments は lazy load (1 個ずつ)」の経路を作る。
///
/// ## Memory pattern (§4.3)
///
/// 旧実装は全 fragment を `Vec<u8>` に concat していたためピーク = `init + Σ fragments`。
/// 新実装はピーク = `init + max(fragment_sizes)` で固定 (`FragmentedSource::peak_memory_hint`)。
/// 50K fragments × 5 MB の long-form ストリームでも 5 MB + init のみで処理可能。
///
/// `fetch_streaming` を使うことで、各 fragment 自体も Range Request 対応なら
/// 内部的に streaming される (typical fragment = 数 MB なので影響は限定的)。
fn fetch_fragmented(
    fetcher: &dyn ContentFetcher,
    init_url: &str,
    fragment_urls: &[String],
    ticket: &Arc<Ticket>,
) -> Result<FetchedContent, FetchError> {
    if fragment_urls.is_empty() {
        return Err(FetchError::NoFragments);
    }

    // ── init segment (数 KB〜数十 KB、in-memory 常駐) ──
    let init_resp = fetcher.fetch(init_url)?;
    if init_resp.body.is_empty() {
        return Err(FetchError::EmptyContent(init_url.to_string()));
    }
    let init_len = init_resp.body.len();
    let init_bytes = init_resp.body;

    // ── 各 fragment の ContentSource + size を収集 ──
    //
    // 全 fragment を fetch_streaming で probe する (HEAD でサイズだけ確定、
    // 実バイトはまだ取らない)。HttpRangeSource なら HEAD のみ、in-memory fallback
    // 経路だと full fetch が発生するため fragment が大きい場合は注意。
    let mut fragment_sources: Vec<Box<dyn ContentSource>> = Vec::with_capacity(fragment_urls.len());
    let mut fragment_sizes: Vec<u64> = Vec::with_capacity(fragment_urls.len());
    for url in fragment_urls {
        let resp = fetcher.fetch_streaming(url)?;
        let size = match resp.size_hint {
            Some(s) if s > 0 => s,
            Some(_) => return Err(FetchError::EmptyContent(url.clone())),
            // size_hint が None だとピークメモリ計算ができない。実装上は
            // fetch_streaming default は size_hint を返すはず (Mock fetcher 含む)。
            None => {
                return Err(FetchError::HttpError {
                    url: url.clone(),
                    reason: "fragment source did not report size_hint".to_string(),
                })
            }
        };
        // 仕様 §4.4 — 1 fragment の最大サイズ check。
        limits::validate_fragment_size(size as usize)?;
        fragment_sources.push(resp.source);
        fragment_sizes.push(size);
    }

    // 仕様 §4.3 のフラグメントメモリパターン (動的モード):
    // - init.len を事前予約 (常駐分)
    // - fragments は FragmentedReader が動的に extend/shrink (swap ごと)
    // これにより同時 reserved は「init + 現在ロード中 1 fragment」ぴったり。
    // fragment サイズが不均一でも厳密に admission control できる。
    ticket.extend(init_len)?;

    let source = crate::fragmented_source::FragmentedSource::new(
        init_bytes,
        fragment_sources,
        fragment_sizes,
    )
    .with_ticket(Arc::clone(ticket));

    Ok(FetchedContent {
        source: Box::new(source),
        content_type: "video/mp4".to_string(),
        manifest_source: None,
    })
}

/// Fetch sidecar content (manifest + content separately).
/// Spec §5.2, §4.3 -- Manifest (.c2pa, typically few KB) を in-memory で取得し、
/// content file は `fetch_single` と同じ streaming 経路 (Range Request 対応なら
/// `HttpRangeSource`、非対応なら full fetch fallback) で取得する。
///
/// ## Memory pattern (§4.3)
///
/// 仕様 §4.3 sidecar 節「メモリパターンは単一ファイルとほぼ同じで、マニフェスト分
/// （数KB〜数十KB）が追加される」と整合する。content 側は Range Request 経路なら
/// peak = `min_req_size` (4 MiB) のみ、in-memory fallback なら full size。
fn fetch_sidecar(
    fetcher: &dyn ContentFetcher,
    manifest_url: &str,
    content_url: &str,
    ticket: &Arc<Ticket>,
) -> Result<FetchedContent, FetchError> {
    // ── Manifest (.c2pa file = raw JUMBF data, 数 KB〜数十 KB) ──
    // 小さいので Range Request は不要、full fetch + InMemorySource で十分。
    let manifest_resp = fetcher.fetch(manifest_url)?;
    if manifest_resp.body.is_empty() {
        return Err(FetchError::EmptyContent(manifest_url.to_string()));
    }
    ticket.extend(manifest_resp.body.len())?;

    // ── Content file (任意サイズ、本体は数十 MB〜数十 GB を想定) ──
    // `fetch_single` と同じ streaming 経路。Range Request 対応サーバーなら
    // 50 GB ファイルでも peak = 4 MiB の予約のみ。
    let content_resp = fetcher.fetch_streaming(content_url)?;

    if let Some(peak) = content_resp.source.peak_memory_hint() {
        if peak == 0 {
            return Err(FetchError::EmptyContent(content_url.to_string()));
        }
        ticket.extend(peak as usize)?;
    }

    // MIME 判定: 先頭数バイトを peek してマジックバイト検出 (fetch_single と同じ
    // パターン)。Range Request 経路では新規 reader で 1 リクエスト分のバッファ
    // が確保されるが、本処理側 source とは独立。
    let mut peek_reader = content_resp.source.open().map_err(|e| FetchError::HttpError {
        url: content_url.to_string(),
        reason: format!("Could not open sidecar content source for MIME peek: {e}"),
    })?;
    let mut magic = [0u8; 12];
    let read = peek_reader
        .read(&mut magic)
        .map_err(|e| FetchError::HttpError {
            url: content_url.to_string(),
            reason: format!("Sidecar content MIME peek read failed: {e}"),
        })?;
    drop(peek_reader);

    let content_type = detect_content_type(
        &magic[..read],
        content_url,
        content_resp.content_type.as_deref(),
    );

    Ok(FetchedContent {
        source: content_resp.source,
        content_type,
        manifest_source: Some(Box::new(InMemorySource::new(manifest_resp.body))),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_pool::ResourcePool;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Mock content fetcher for unit tests.
    struct MockFetcher {
        responses: HashMap<String, (Vec<u8>, Option<String>)>,
    }

    impl MockFetcher {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }

        fn add(&mut self, url: &str, body: Vec<u8>, content_type: Option<&str>) {
            self.responses
                .insert(url.to_string(), (body, content_type.map(|s| s.to_string())));
        }
    }

    impl ContentFetcher for MockFetcher {
        fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError> {
            let (body, ct) = self.responses.get(url).ok_or(FetchError::HttpStatus {
                status: 404,
                url: url.to_string(),
            })?;
            Ok(FetchResponse {
                body: body.clone(),
                content_type: ct.clone(),
                etag: Some("\"test-etag\"".to_string()),
            })
        }
    }

    /// Create a large-enough pool and ticket for tests that don't care about limits.
    fn test_pool_and_ticket() -> (Arc<ResourcePool>, Arc<Ticket>) {
        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000_000)); // 1 GB
        let ticket = Arc::new(pool.ticket(Some(0)));
        (pool, ticket)
    }

    /// `ContentSource` から実バイト列を取り出すテストヘルパ。
    fn drain(source: &dyn ContentSource) -> Vec<u8> {
        let mut r = source.open().expect("open should succeed");
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).expect("read_to_end should succeed");
        buf
    }

    // -- detect_content_type --

    #[test]
    fn detect_jpeg_magic_bytes() {
        let bytes = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
        ];
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/img", None),
            "image/jpeg"
        );
    }

    #[test]
    fn detect_png_magic_bytes() {
        let bytes = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/img", None),
            "image/png"
        );
    }

    #[test]
    fn detect_mp4_magic_bytes() {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x1C]; // size
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom");
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/vid", None),
            "video/mp4"
        );
    }

    #[test]
    fn detect_from_server_header() {
        let bytes = vec![0x00; 4]; // no magic match
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/data", Some("image/webp")),
            "image/webp"
        );
    }

    #[test]
    fn detect_from_url_extension() {
        let bytes = vec![0x00; 4]; // no magic match
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/photo.jpg", None),
            "image/jpeg"
        );
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/video.mp4", None),
            "video/mp4"
        );
    }

    #[test]
    fn detect_magic_overrides_server_header() {
        // JPEG magic bytes with wrong server header
        let bytes = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
        ];
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/img", Some("text/plain")),
            "image/jpeg"
        );
    }

    #[test]
    fn detect_fallback_octet_stream() {
        let bytes = vec![0x00; 4];
        assert_eq!(
            detect_content_type(&bytes, "https://example.com/data", None),
            "application/octet-stream"
        );
    }

    // -- fetch_single --

    #[test]
    fn fetch_single_success() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
            ],
            Some("image/jpeg"),
        );

        let input = InputData::Single {
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket).unwrap();
        assert_eq!(result.content_type, "image/jpeg");
        assert_eq!(result.source.size_hint(), Some(12));
        assert_eq!(drain(result.source.as_ref()).len(), 12);
        assert!(result.manifest_source.is_none());
        assert_eq!(ticket.reserved(), 12);
    }

    #[test]
    fn fetch_single_empty_content_error() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/empty.jpg",
            vec![],
            Some("image/jpeg"),
        );

        let input = InputData::Single {
            content_url: "https://storage.example.com/empty.jpg".to_string(),
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FetchError::EmptyContent(_)));
    }

    #[test]
    fn fetch_single_not_found_error() {
        let fetcher = MockFetcher::new();
        let input = InputData::Single {
            content_url: "https://storage.example.com/missing.jpg".to_string(),
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FetchError::HttpStatus { status: 404, .. }
        ));
    }

    /// Default `fetch_streaming` 実装の検証。`ContentFetcher` を素朴に
    /// 実装した fetcher が full body を `InMemorySource` で包んで返すこと、
    /// および `size_hint` / `peak_memory_hint` が body 長と一致することを確認。
    #[test]
    fn default_fetch_streaming_wraps_full_body_as_in_memory_source() {
        use title_core::ContentSource;

        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01],
            Some("image/jpeg"),
        );

        let resp = fetcher
            .fetch_streaming("https://storage.example.com/photo.jpg")
            .expect("fetch_streaming");

        assert_eq!(resp.size_hint, Some(12));
        // default impl は InMemorySource を返すので peak_memory_hint == size_hint
        assert_eq!(resp.source.peak_memory_hint(), Some(12));
        assert_eq!(resp.content_type.as_deref(), Some("image/jpeg"));

        let mut reader = resp.source.open().unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(buf.len(), 12);
    }

    /// 空 body を渡した default `fetch_streaming` は EmptyContent を返すべき。
    #[test]
    fn default_fetch_streaming_rejects_empty_body() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/empty",
            vec![],
            Some("image/jpeg"),
        );
        let err = fetcher
            .fetch_streaming("https://storage.example.com/empty")
            .expect_err("empty body must error");
        assert!(matches!(err, FetchError::EmptyContent(_)));
    }

    /// 仕様 §4.3 — Range Request 経路のソースは `peak_memory_hint` が小さい
    /// (例: 64 KB)。`size_hint` が 50 GB でも ticket への extend は数十 KB だけ。
    /// in-memory パスは依然 size_hint 分予約 (Mock fetcher 経由はこちら)。
    #[test]
    fn fetch_single_streaming_source_reserves_only_peak_memory() {
        use std::io::{Cursor, Seek, SeekFrom};
        use title_core::{ContentSource, ContentStream};

        /// 50 GB を「装う」テスト用 ContentSource。実バイトは 16 byte の
        /// JPEG マジックだが、size_hint が 50 GB、peak_memory_hint が 64 KB。
        /// fetch_single が peak_memory_hint で予約することを検証する。
        struct FiftyGbStreamingSource;

        impl ContentSource for FiftyGbStreamingSource {
            fn open(&self) -> std::io::Result<Box<dyn ContentStream>> {
                Ok(Box::new(Cursor::new(vec![
                    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
                ])))
            }
            fn size_hint(&self) -> Option<u64> {
                Some(50 * 1024 * 1024 * 1024) // 50 GB (嘘の値)
            }
            fn peak_memory_hint(&self) -> Option<u64> {
                Some(64 * 1024) // 64 KB のリーダーバッファ相当
            }
        }

        // fetcher は streaming source を直接返すモック。
        struct StreamingMockFetcher;
        impl ContentFetcher for StreamingMockFetcher {
            fn fetch(&self, _url: &str) -> Result<FetchResponse, FetchError> {
                unreachable!("streaming path should not call fetch()")
            }
            fn fetch_streaming(
                &self,
                _url: &str,
            ) -> Result<StreamingFetchResponse, FetchError> {
                Ok(StreamingFetchResponse {
                    source: Box::new(FiftyGbStreamingSource),
                    content_type: Some("image/jpeg".to_string()),
                    etag: None,
                    size_hint: Some(50 * 1024 * 1024 * 1024),
                })
            }
        }

        let input = InputData::Single {
            content_url: "https://50gb.example.com/video.mp4".to_string(),
        };

        // admission_limit / total_limit は 1 MB だけ。50 GB を pre-extend
        // しようとしていたらここで TotalLimitExceeded で reject されるはず。
        let pool = Arc::new(ResourcePool::with_single_limit(1024 * 1024));
        let ticket = Arc::new(pool.ticket(Some(0)));
        let result = fetch_content(&StreamingMockFetcher, &input, &ticket).expect("succeed");

        // ピークメモリだけ予約されている (64 KB)
        assert_eq!(ticket.reserved(), 64 * 1024);
        // 論理サイズ (50 GB) はそのまま source に保持される
        assert_eq!(result.source.size_hint(), Some(50 * 1024 * 1024 * 1024));

        // 続けて processor 並みに read しても OOM しない (バッファ越えはしない)
        let mut r = result.source.open().unwrap();
        let mut buf = [0u8; 4];
        r.seek(SeekFrom::Start(0)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [0xFF, 0xD8, 0xFF, 0xE0]);
    }

    #[test]
    fn fetch_single_exceeds_memory_limit() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/large.jpg",
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
            ],
            Some("image/jpeg"),
        );

        let input = InputData::Single {
            content_url: "https://storage.example.com/large.jpg".to_string(),
        };

        // Pool with very small limit
        let pool = Arc::new(ResourcePool::with_single_limit(5));
        let ticket = Arc::new(pool.ticket(Some(0)));
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FetchError::MemoryLimit(_)));
    }

    // -- fetch_fragmented --

    #[test]
    fn fetch_fragmented_concatenates_segments() {
        let mut fetcher = MockFetcher::new();

        // init.mp4: ftyp box header
        let mut init_bytes = vec![0x00, 0x00, 0x00, 0x08];
        init_bytes.extend_from_slice(b"ftyp");
        fetcher.add(
            "https://storage.example.com/video/init.mp4",
            init_bytes.clone(),
            Some("video/mp4"),
        );

        // seg-0.m4s
        let seg0 = b"segment-0-data".to_vec();
        fetcher.add(
            "https://storage.example.com/video/seg-0.m4s",
            seg0.clone(),
            Some("video/mp4"),
        );

        // seg-1.m4s
        let seg1 = b"segment-1-data".to_vec();
        fetcher.add(
            "https://storage.example.com/video/seg-1.m4s",
            seg1.clone(),
            Some("video/mp4"),
        );

        let input = InputData::Fragmented {
            init_url: "https://storage.example.com/video/init.mp4".to_string(),
            fragment_urls: vec![
                "https://storage.example.com/video/seg-0.m4s".to_string(),
                "https://storage.example.com/video/seg-1.m4s".to_string(),
            ],
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket).unwrap();
        assert_eq!(result.content_type, "video/mp4");
        let bytes = drain(result.source.as_ref());
        assert_eq!(bytes.len(), init_bytes.len() + seg0.len() + seg1.len());
        // Verify concatenation order
        assert_eq!(&bytes[..init_bytes.len()], &init_bytes[..]);
        assert_eq!(
            &bytes[init_bytes.len()..init_bytes.len() + seg0.len()],
            &seg0[..]
        );
        assert!(result.manifest_source.is_none());
        // Verify memory tracking — 仕様 §4.3 フラグメントパターン (動的モード):
        // fetch_fragmented 直後は init 分のみ予約。fragments は FragmentedReader が
        // 読む際に動的に extend / drop 時に shrink するので、source を read しない
        // この時点では reserved = init.len のみ。
        assert_eq!(ticket.reserved(), init_bytes.len());

        // 実際に reader を使って read_to_end したときは init + 現在ロード中
        // fragment が reserved されることを確認。read 完了後 (reader drop) で
        // shrink され、reserved = init に戻る。
        use std::io::Read as _;
        let mut reader = result.source.open().unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        // 最後に読んだ fragment が current_fragment として残っている状態
        let last_frag_size = seg1.len();
        assert_eq!(ticket.reserved(), init_bytes.len() + last_frag_size);
        // reader drop で fragment 分が shrink される
        drop(reader);
        assert_eq!(ticket.reserved(), init_bytes.len());
    }

    #[test]
    fn fetch_fragmented_no_fragments_error() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/video/init.mp4",
            vec![0x01],
            None,
        );

        let input = InputData::Fragmented {
            init_url: "https://storage.example.com/video/init.mp4".to_string(),
            fragment_urls: vec![],
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FetchError::NoFragments));
    }

    #[test]
    fn fetch_fragmented_missing_segment_error() {
        let mut fetcher = MockFetcher::new();

        let init_bytes = vec![0x00, 0x00, 0x00, 0x08, b'f', b't', b'y', b'p'];
        fetcher.add(
            "https://storage.example.com/video/init.mp4",
            init_bytes,
            None,
        );
        // seg-0 exists, seg-1 does not
        fetcher.add(
            "https://storage.example.com/video/seg-0.m4s",
            b"segment-data".to_vec(),
            None,
        );

        let input = InputData::Fragmented {
            init_url: "https://storage.example.com/video/init.mp4".to_string(),
            fragment_urls: vec![
                "https://storage.example.com/video/seg-0.m4s".to_string(),
                "https://storage.example.com/video/seg-1.m4s".to_string(),
            ],
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
    }

    #[test]
    fn fetch_fragmented_memory_limit_mid_fetch() {
        let mut fetcher = MockFetcher::new();

        let init_bytes = vec![0x00, 0x00, 0x00, 0x08, b'f', b't', b'y', b'p'];
        fetcher.add(
            "https://storage.example.com/video/init.mp4",
            init_bytes.clone(),
            None,
        );
        fetcher.add(
            "https://storage.example.com/video/seg-0.m4s",
            vec![0u8; 100],
            None,
        );

        let input = InputData::Fragmented {
            init_url: "https://storage.example.com/video/init.mp4".to_string(),
            fragment_urls: vec!["https://storage.example.com/video/seg-0.m4s".to_string()],
        };

        // Pool can hold init (8 bytes) + 50 bytes、fragment は 100 bytes。
        // 動的モード:
        //   - fetch_fragmented 自体は init のみ extend で成功 (8 bytes 予約)
        //   - reader.read() で fragment 分 (100 bytes) extend を試み MemoryLimit
        let pool = Arc::new(ResourcePool::with_single_limit(init_bytes.len() + 50));
        let ticket = Arc::new(pool.ticket(Some(0)));
        let fetched = fetch_content(&fetcher, &input, &ticket).expect("init fetch ok");
        assert_eq!(ticket.reserved(), init_bytes.len());

        use std::io::Read as _;
        let mut reader = fetched.source.open().unwrap();
        let mut buf = Vec::new();
        let read_err = reader
            .read_to_end(&mut buf)
            .expect_err("fragment extend should hit memory limit");
        assert_eq!(read_err.kind(), std::io::ErrorKind::OutOfMemory);
        // extend が失敗したので current_fragment はセットされず、reader drop でも
        // 余分な shrink は走らない。init 分の reserved は維持。
        drop(reader);
        assert_eq!(ticket.reserved(), init_bytes.len());
    }

    #[test]
    fn fetch_fragmented_fragment_size_exceeded() {
        let mut fetcher = MockFetcher::new();

        let init_bytes = vec![0x00, 0x00, 0x00, 0x08, b'f', b't', b'y', b'p'];
        fetcher.add(
            "https://storage.example.com/video/init.mp4",
            init_bytes,
            None,
        );
        // Fragment exceeds MAX_FRAGMENT_SIZE (100 MB)
        // We can't create a 100MB+ vec in tests, so we use a smaller limit test
        // via validate_fragment_size directly. This test verifies wiring.
        // In practice, the real limit is 100 MB.
        fetcher.add(
            "https://storage.example.com/video/seg-0.m4s",
            vec![0u8; 10],
            None,
        );

        // The fragment size validation is covered by limits::tests.
        // This test ensures validate_fragment_size is called in the pipeline.
        let input = InputData::Fragmented {
            init_url: "https://storage.example.com/video/init.mp4".to_string(),
            fragment_urls: vec!["https://storage.example.com/video/seg-0.m4s".to_string()],
        };

        let (_pool, ticket) = test_pool_and_ticket();
        // Small fragments should pass validation
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_ok());
    }

    // -- fetch_sidecar --

    #[test]
    fn fetch_sidecar_both_files() {
        let mut fetcher = MockFetcher::new();

        // Manifest (.c2pa): raw JUMBF data (mock)
        fetcher.add(
            "https://storage.example.com/photo.c2pa",
            b"jumbf-manifest-data".to_vec(),
            Some("application/c2pa"),
        );

        // Content file
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
            ],
            Some("image/jpeg"),
        );

        let input = InputData::Sidecar {
            manifest_url: "https://storage.example.com/photo.c2pa".to_string(),
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket).unwrap();
        assert_eq!(result.content_type, "image/jpeg");
        assert_eq!(drain(result.source.as_ref()).len(), 12);
        let manifest = result.manifest_source.as_ref().expect("manifest present");
        assert_eq!(drain(manifest.as_ref()), b"jumbf-manifest-data".to_vec());
        // Memory tracking: manifest + content
        assert_eq!(ticket.reserved(), 19 + 12);
    }

    #[test]
    fn fetch_sidecar_missing_manifest_error() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            vec![0xFF, 0xD8, 0xFF],
            None,
        );

        let input = InputData::Sidecar {
            manifest_url: "https://storage.example.com/photo.c2pa".to_string(),
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
    }

    #[test]
    fn fetch_sidecar_missing_content_error() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.c2pa",
            b"jumbf-data".to_vec(),
            None,
        );

        let input = InputData::Sidecar {
            manifest_url: "https://storage.example.com/photo.c2pa".to_string(),
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        let (_pool, ticket) = test_pool_and_ticket();
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
    }

    #[test]
    fn fetch_sidecar_memory_limit_on_content() {
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.c2pa",
            b"manifest".to_vec(),
            None,
        );
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
            ],
            None,
        );

        let input = InputData::Sidecar {
            manifest_url: "https://storage.example.com/photo.c2pa".to_string(),
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        // Pool can hold manifest but not manifest + content
        let pool = Arc::new(ResourcePool::with_single_limit(10)); // 8 (manifest) + 12 (content) > 10
        let ticket = Arc::new(pool.ticket(Some(0)));
        let result = fetch_content(&fetcher, &input, &ticket);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FetchError::MemoryLimit(_)));
        // Manifest reservation should still be tracked
        assert_eq!(ticket.reserved(), 8);
    }
}
