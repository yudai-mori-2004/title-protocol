// SPDX-License-Identifier: Apache-2.0

//! # Content Fetch Layer
//!
//! Spec SS5.2 -- Content fetch from external storage
//!
//! Fetches content from URL(s) based on input type (single / fragmented / sidecar).
//! Provides a trait-based abstraction over the HTTP client, enabling mock-based
//! testing without network I/O.
//!
//! ## Input types
//!
//! | Type | Fetch strategy |
//! |---|---|
//! | Single | HTTP GET for the content file |
//! | Fragmented | Sequential HTTP GET: init.mp4, then each seg-*.m4s |
//! | Sidecar | Two HTTP GETs: manifest (.c2pa) + content file |
//!
//! ## ETag consistency (SS5.2)
//!
//! For Range Request scenarios (future optimization), the initial ETag is
//! recorded and sent in subsequent If-Match headers. A 412 response means
//! the file changed during transfer, and the request is aborted.

use title_core::InputData;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error during content fetching.
/// Spec SS5.2
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
    /// Spec SS5.2 -- 412 Precondition Failed
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
}

// ---------------------------------------------------------------------------
// Fetcher trait and types
// ---------------------------------------------------------------------------

/// Response from a single HTTP fetch.
pub struct FetchResponse {
    /// Response body bytes.
    pub body: Vec<u8>,
    /// Content-Type header value, if present.
    pub content_type: Option<String>,
    /// ETag header value, if present.
    pub etag: Option<String>,
}

/// Content fetcher trait.
/// Spec SS5.2
///
/// Abstraction over the HTTP client for testability.
/// The orchestrator depends on this trait, not on `reqwest` directly.
pub trait ContentFetcher: Send + Sync {
    /// Fetch the full body from a URL.
    ///
    /// Implementations should follow redirects and return the final body.
    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError>;
}

/// HTTP-based content fetcher using `reqwest::blocking::Client`.
/// Spec SS5.2
pub struct HttpContentFetcher {
    client: reqwest::blocking::Client,
}

impl HttpContentFetcher {
    /// Creates a new HTTP content fetcher with default settings.
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .user_agent("title-tee/0.1.2")
                .build()
                .expect("Failed to build HTTP client"),
        }
    }
}

impl Default for HttpContentFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentFetcher for HttpContentFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError> {
        let resp = self.client.get(url).send().map_err(|e| FetchError::HttpError {
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

        let body = resp.bytes().map_err(|e| FetchError::HttpError {
            url: url.to_string(),
            reason: format!("Body read error: {e}"),
        })?;

        Ok(FetchResponse {
            body: body.to_vec(),
            content_type,
            etag,
        })
    }
}

// ---------------------------------------------------------------------------
// Fetched content
// ---------------------------------------------------------------------------

/// Fetched content, normalized for processor consumption.
/// Spec SS5.2
#[derive(Debug)]
pub struct FetchedContent {
    /// Content bytes.
    /// - Single: the file bytes.
    /// - Fragmented: init.mp4 + all segments concatenated into a contiguous
    ///   buffer. BMFF fragmented MP4 is a sequence of boxes
    ///   (ftyp+moov from init, moof+mdat from each segment), so simple
    ///   concatenation produces a valid container.
    /// - Sidecar: the content file bytes (manifest is in `manifest_bytes`).
    pub content_bytes: Vec<u8>,

    /// Detected MIME type of the content.
    pub content_type: String,

    /// For sidecar input: the separate manifest (.c2pa) bytes containing
    /// raw JUMBF data. `None` for single and fragmented inputs.
    pub manifest_bytes: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Content type detection
// ---------------------------------------------------------------------------

/// Detect MIME type from magic bytes, server Content-Type, and URL extension.
/// Priority: magic bytes > server header > URL extension > fallback.
fn detect_content_type(bytes: &[u8], url: &str, server_type: Option<&str>) -> String {
    // Magic bytes (highest priority -- most reliable)
    if bytes.len() >= 12 {
        // JPEG: FF D8 FF
        if bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            return "image/jpeg".to_string();
        }
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        if bytes.len() >= 8
            && bytes[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        {
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

    "application/octet-stream".to_string()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch content based on input data type.
/// Spec SS5.2 -- dispatches to the appropriate fetch strategy.
pub fn fetch_content(
    fetcher: &dyn ContentFetcher,
    input: &InputData,
) -> Result<FetchedContent, FetchError> {
    match input {
        InputData::Single { content_url } => fetch_single(fetcher, content_url),
        InputData::Fragmented {
            init_url,
            fragment_urls,
        } => fetch_fragmented(fetcher, init_url, fragment_urls),
        InputData::Sidecar {
            manifest_url,
            content_url,
        } => fetch_sidecar(fetcher, manifest_url, content_url),
    }
}

/// Fetch single file content.
/// Spec SS5.2 -- HTTP GET for the content URL.
fn fetch_single(fetcher: &dyn ContentFetcher, url: &str) -> Result<FetchedContent, FetchError> {
    let resp = fetcher.fetch(url)?;
    if resp.body.is_empty() {
        return Err(FetchError::EmptyContent(url.to_string()));
    }

    let content_type = detect_content_type(&resp.body, url, resp.content_type.as_deref());

    Ok(FetchedContent {
        content_bytes: resp.body,
        content_type,
        manifest_bytes: None,
    })
}

/// Fetch fragmented content (CMAF init + segments).
/// Spec SS5.2 -- Sequential HTTP GET for init.mp4 then each seg-*.m4s.
///
/// BMFF/ISO-14496-12 fragmented MP4 is a sequence of boxes:
/// ftyp + moov (from init.mp4) followed by moof + mdat (from each segment).
/// Simple byte concatenation produces a valid fragmented MP4 container.
fn fetch_fragmented(
    fetcher: &dyn ContentFetcher,
    init_url: &str,
    fragment_urls: &[String],
) -> Result<FetchedContent, FetchError> {
    if fragment_urls.is_empty() {
        return Err(FetchError::NoFragments);
    }

    // Fetch init segment
    let init_resp = fetcher.fetch(init_url)?;
    if init_resp.body.is_empty() {
        return Err(FetchError::EmptyContent(init_url.to_string()));
    }

    // Pre-allocate with estimated capacity
    let estimated_size = init_resp.body.len() + fragment_urls.len() * 512 * 1024;
    let mut combined = Vec::with_capacity(estimated_size);
    combined.extend_from_slice(&init_resp.body);

    // Fetch and concatenate each fragment segment
    for fragment_url in fragment_urls {
        let frag_resp = fetcher.fetch(fragment_url)?;
        if frag_resp.body.is_empty() {
            return Err(FetchError::EmptyContent(fragment_url.clone()));
        }
        combined.extend_from_slice(&frag_resp.body);
    }

    Ok(FetchedContent {
        content_bytes: combined,
        content_type: "video/mp4".to_string(),
        manifest_bytes: None,
    })
}

/// Fetch sidecar content (manifest + content separately).
/// Spec SS5.2 -- Two HTTP GETs for the manifest (.c2pa) and content file.
fn fetch_sidecar(
    fetcher: &dyn ContentFetcher,
    manifest_url: &str,
    content_url: &str,
) -> Result<FetchedContent, FetchError> {
    // Fetch manifest (.c2pa file = raw JUMBF data)
    let manifest_resp = fetcher.fetch(manifest_url)?;
    if manifest_resp.body.is_empty() {
        return Err(FetchError::EmptyContent(manifest_url.to_string()));
    }

    // Fetch content file
    let content_resp = fetcher.fetch(content_url)?;
    if content_resp.body.is_empty() {
        return Err(FetchError::EmptyContent(content_url.to_string()));
    }

    let content_type =
        detect_content_type(&content_resp.body, content_url, content_resp.content_type.as_deref());

    Ok(FetchedContent {
        content_bytes: content_resp.body,
        content_type,
        manifest_bytes: Some(manifest_resp.body),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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

    // -- detect_content_type --

    #[test]
    fn detect_jpeg_magic_bytes() {
        let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01];
        assert_eq!(detect_content_type(&bytes, "https://example.com/img", None), "image/jpeg");
    }

    #[test]
    fn detect_png_magic_bytes() {
        let bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D];
        assert_eq!(detect_content_type(&bytes, "https://example.com/img", None), "image/png");
    }

    #[test]
    fn detect_mp4_magic_bytes() {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x1C]; // size
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom");
        assert_eq!(detect_content_type(&bytes, "https://example.com/vid", None), "video/mp4");
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
        let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01];
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
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01],
            Some("image/jpeg"),
        );

        let input = InputData::Single {
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        let result = fetch_content(&fetcher, &input).unwrap();
        assert_eq!(result.content_type, "image/jpeg");
        assert_eq!(result.content_bytes.len(), 12);
        assert!(result.manifest_bytes.is_none());
    }

    #[test]
    fn fetch_single_empty_content_error() {
        let mut fetcher = MockFetcher::new();
        fetcher.add("https://storage.example.com/empty.jpg", vec![], Some("image/jpeg"));

        let input = InputData::Single {
            content_url: "https://storage.example.com/empty.jpg".to_string(),
        };

        let result = fetch_content(&fetcher, &input);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FetchError::EmptyContent(_)));
    }

    #[test]
    fn fetch_single_not_found_error() {
        let fetcher = MockFetcher::new();
        let input = InputData::Single {
            content_url: "https://storage.example.com/missing.jpg".to_string(),
        };

        let result = fetch_content(&fetcher, &input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FetchError::HttpStatus { status: 404, .. }
        ));
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

        let result = fetch_content(&fetcher, &input).unwrap();
        assert_eq!(result.content_type, "video/mp4");
        assert_eq!(
            result.content_bytes.len(),
            init_bytes.len() + seg0.len() + seg1.len()
        );
        // Verify concatenation order
        assert_eq!(&result.content_bytes[..init_bytes.len()], &init_bytes[..]);
        assert_eq!(
            &result.content_bytes[init_bytes.len()..init_bytes.len() + seg0.len()],
            &seg0[..]
        );
        assert!(result.manifest_bytes.is_none());
    }

    #[test]
    fn fetch_fragmented_no_fragments_error() {
        let mut fetcher = MockFetcher::new();
        fetcher.add("https://storage.example.com/video/init.mp4", vec![0x01], None);

        let input = InputData::Fragmented {
            init_url: "https://storage.example.com/video/init.mp4".to_string(),
            fragment_urls: vec![],
        };

        let result = fetch_content(&fetcher, &input);
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

        let result = fetch_content(&fetcher, &input);
        assert!(result.is_err());
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
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01],
            Some("image/jpeg"),
        );

        let input = InputData::Sidecar {
            manifest_url: "https://storage.example.com/photo.c2pa".to_string(),
            content_url: "https://storage.example.com/photo.jpg".to_string(),
        };

        let result = fetch_content(&fetcher, &input).unwrap();
        assert_eq!(result.content_type, "image/jpeg");
        assert_eq!(result.content_bytes.len(), 12);
        assert!(result.manifest_bytes.is_some());
        assert_eq!(
            result.manifest_bytes.as_ref().unwrap(),
            b"jumbf-manifest-data"
        );
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

        let result = fetch_content(&fetcher, &input);
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

        let result = fetch_content(&fetcher, &input);
        assert!(result.is_err());
    }
}
