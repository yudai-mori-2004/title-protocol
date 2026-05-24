// SPDX-License-Identifier: Apache-2.0

//! # TEE Request Processing Orchestrator
//!
//! Spec §5.2 -- TEE request processing flow
//!
//! Implements the full pipeline from `ProcessRequest` to `ProcessResponse`:
//!
//! 0. Pre-flight validation (reject encryption × non-`Single` input early)
//! 1. Admit request (§4.1 -- ResourcePool admission check)
//! 2. Fetch content from URL(s) based on input type (with memory tracking)
//! 3. Decrypt the fetched payload when `request.encryption` is set (§2.4)
//! 4. Compute `signature_hash` (§1.3 -- mandatory for all requests)
//! 5. For encrypted requests, verify the inner `signature_hash` matches (§2.4)
//! 6. Ensure `c2pa-verify` is in the processor list (§1.3 -- implicitly required)
//! 7. Execute processors via `ProcessorRegistry` (§3.1)
//! 8. Assemble + JCS-canonicalize + SHA-256 (§1.5, §2.3), wrap in attestation (§1.2)
//! 9. For encrypted requests, seal the response with the negotiated key (§2.4)
//!
//! ## Sidecar handling
//!
//! For sidecar inputs, the manifest (.c2pa) is separate from the content file.
//! The orchestrator computes `signature_hash` from the manifest JUMBF data
//! directly, and runs `c2pa-verify` on the content bytes (which may fail if
//! the content has no embedded manifest). Other processors receive the raw
//! content bytes.
//!
//! ## Memory management (§4.1, §4.2)
//!
//! Each request gets a Ticket from the ResourcePool. Memory is tracked
//! throughout the pipeline via Ticket.extend() calls in the content fetch
//! layer. The Ticket is dropped at the end of the function, releasing all
//! reserved memory.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use sha2::{Digest, Sha256};

use title_core::{
    compute_signature_hash, compute_signature_hash_from_manifest_data, EncryptionSuite, InputData,
    ProcessRequest, ProcessResponse, ProcessorOutput, ProcessorRegistry, VerifiableResponse,
    C2PA_VERIFY_PROCESSOR_ID,
};
use title_crypto::key_bundle::KeyBundle;
use title_crypto::payload;
use title_crypto::sealed_channel::{open_request, ResponseChannel};

use crate::content_fetch::{detect_content_type, fetch_content, ContentFetcher, FetchError};
use crate::resource_pool::ResourcePool;
use crate::TeeRuntime;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Orchestrator error.
/// Spec §5.2
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    /// Request rejected: ResourcePool admission limit exceeded.
    /// Spec §4.1 -- corresponds to HTTP 503.
    #[error("Request rejected: memory admission limit exceeded (503)")]
    AdmissionRejected,

    /// Content fetch failed.
    #[error("Content fetch failed: {0}")]
    FetchFailed(#[from] FetchError),

    /// signature_hash computation failed.
    /// This means the content has no valid C2PA signature -- the request
    /// is rejected because Title Protocol requires C2PA-signed content.
    /// Spec §3.1 -- "C2PA署名のないコンテンツに対してはリクエスト全体が拒否される"
    #[error("Failed to compute signature_hash: {0}")]
    SignatureHashFailed(String),

    /// JCS canonicalization failed.
    #[error("JCS canonicalization failed: {0}")]
    JcsFailed(String),

    /// Attestation Document retrieval failed.
    #[error("Attestation document retrieval failed: {0}")]
    AttestationFailed(String),

    /// JSON serialization error.
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Encryption is requested but for an input type whose encrypted form is
    /// not implemented. Spec §2.4 defines the encrypted wire payload only
    /// for `input_type: "single"`; `fragmented` / `sidecar` encryption is
    /// a future-protocol concern, not a current capability gap.
    #[error(
        "encrypted requests require input_type=\"single\" (fragmented/sidecar encryption is not implemented)"
    )]
    EncryptionRequiresSingleInput,

    /// Decryption (KEM / HKDF / AES-GCM) of the fetched payload failed.
    #[error("payload decryption failed: {0}")]
    DecryptionFailed(String),

    /// The encryption suite declared on `ProcessRequest.encryption` does
    /// not match the suite encoded in the wire payload header.
    #[error(
        "encryption suite mismatch: request declared {declared:?}, wire payload says {wire:?}"
    )]
    EncryptionSuiteMismatch {
        declared: EncryptionSuite,
        wire: EncryptionSuite,
    },

    /// Decrypted payload's metadata header could not be parsed or did not
    /// contain the expected fields.
    #[error("encrypted payload metadata invalid: {0}")]
    PayloadMetadataInvalid(String),

    /// The signature_hash declared inside the encrypted payload does not
    /// match the value computed from the actual content. Spec §2.4 step 8.
    #[error("signature_hash mismatch between encrypted payload metadata and decrypted content")]
    SignatureHashMismatch,

    /// Encrypting the response with the negotiated response key failed.
    #[error("response encryption failed: {0}")]
    ResponseSealFailed(String),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// The TEE's response to a `ProcessRequest`.
///
/// Plaintext requests return a JSON [`ProcessResponse`]; encrypted requests
/// return the wire-format ciphertext (Spec §2.4 response wire format:
/// `nonce(12B) || ciphertext`) that only the requesting client can decrypt.
#[derive(Debug)]
pub enum ProcessOutcome {
    Plaintext(ProcessResponse),
    Encrypted(Vec<u8>),
}

/// Process a request through the full TEE pipeline.
/// Spec §5.2 — TEE request processing flow
///
/// # Arguments
/// * `request`      — Client request (with optional `encryption` field, §2.4)
/// * `fetcher`      — Content fetcher (HTTP client or mock)
/// * `registry`     — Processor registry with registered processors
/// * `runtime`      — TEE runtime for Attestation Document retrieval
/// * `pool`         — ResourcePool for memory management (§4.1)
/// * `key_bundle`   — TEE's per-suite encryption key bundle (§2.4).
///   Only used when `request.encryption` is set.
///
/// # Errors
/// - `AdmissionRejected` — ResourcePool admission limit exceeded
/// - Content fetch failure (network, HTTP error, empty content, memory limit)
/// - For encrypted requests: decryption / payload-format / signature_hash
///   mismatch failures (§2.4)
/// - No valid C2PA signature in content (signature_hash computation fails)
/// - JCS canonicalization failure
/// - Attestation Document retrieval failure
pub fn process_request(
    request: &ProcessRequest,
    fetcher: &dyn ContentFetcher,
    registry: &ProcessorRegistry,
    runtime: &dyn TeeRuntime,
    pool: &Arc<ResourcePool>,
    key_bundle: &KeyBundle,
) -> Result<ProcessOutcome, OrchestratorError> {
    // Step 0: Reject incompatible encryption + input combinations before
    // touching the network. Spec §2.4 — only `Single` inputs carry the
    // encrypted wire payload; for `Fragmented`/`Sidecar` we would otherwise
    // fetch every URL only to bail out on decrypt.
    if request.encryption.is_some() && !matches!(request.input, InputData::Single { .. }) {
        return Err(OrchestratorError::EncryptionRequiresSingleInput);
    }

    // Step 1: Admit request. Spec §4.1.
    // Hint the timeout calculation with `Single` = unknown (None → MAX),
    // `Fragmented` = sum of fragment sizes (none recorded up front, so we
    // hint with `max_fragment_size × count` so the global timeout matches
    // the worst case the request could legitimately occupy).
    let size_hint: Option<u64> = match &request.input {
        InputData::Single { .. } | InputData::Sidecar { .. } => None,
        InputData::Fragmented {
            init_url: _,
            fragment_urls,
        } => Some(
            (fragment_urls.len() as u64).saturating_mul(crate::limits::MAX_FRAGMENT_SIZE as u64),
        ),
    };
    let ticket = pool
        .try_admit(size_hint)
        .ok_or(OrchestratorError::AdmissionRejected)?;

    // Step 2: Fetch content from URL(s) with memory tracking
    // Spec §5.2, §4.2
    let fetched = fetch_content(fetcher, &request.input, &ticket)?;

    // Step 3: Decrypt if the request declares an encryption suite (§2.4).
    // The fetched body is the encrypted wire payload; after decryption we
    // obtain `metadata + raw content` and the response channel used to seal
    // the eventual reply.
    let (content_bytes, content_type, manifest_bytes, response_channel, declared_signature_hash) =
        if let Some(suite) = request.encryption {
            decrypt_single_payload(suite, &request.input, &fetched, key_bundle)?
        } else {
            (
                fetched.content_bytes,
                fetched.content_type,
                fetched.manifest_bytes,
                None,
                None,
            )
        };

    // Step 4: Compute signature_hash from the (decrypted) content/manifest.
    // Spec §1.3 — SHA-256(Active Manifest's COSE signature)
    let signature_hash = if let Some(ref manifest_data) = manifest_bytes {
        compute_signature_hash_from_manifest_data(manifest_data)
            .map_err(|e| OrchestratorError::SignatureHashFailed(e.to_string()))?
    } else {
        compute_signature_hash(&content_bytes, &content_type)
            .map_err(|e| OrchestratorError::SignatureHashFailed(e.to_string()))?
    };

    // Step 5: For encrypted requests, verify the declared signature_hash
    // matches what we just computed (§2.4 step 8).
    if let Some(declared) = declared_signature_hash {
        if declared != signature_hash {
            return Err(OrchestratorError::SignatureHashMismatch);
        }
    }

    // Step 6: Build processor ID list with c2pa-verify always included
    let processor_ids = ensure_c2pa_verify(&request.processor_ids);

    // Step 7: Execute processors via registry
    let results = execute_processors(registry, &processor_ids, &content_bytes, &content_type);

    let verifiable = VerifiableResponse {
        signature_hash,
        results,
    };

    // Step 8: JCS hash + Attestation Document + assembled ProcessResponse.
    let response = build_attested_response(verifiable, runtime)?;

    // Step 9: Seal the response if this was an encrypted request, else
    // return the plaintext JSON response unchanged.
    match response_channel {
        None => Ok(ProcessOutcome::Plaintext(response)),
        Some(channel) => {
            let response_json = serde_json::to_vec(&response)?;
            let wire = channel
                .seal(&response_json)
                .map_err(|e| OrchestratorError::ResponseSealFailed(format!("{e:?}")))?;
            Ok(ProcessOutcome::Encrypted(wire))
        }
    }
}

/// Decrypt a single-file encrypted payload and surface its inner content.
///
/// Returns `(content_bytes, content_type, manifest_bytes, response_channel,
/// declared_signature_hash)`. `manifest_bytes` is always `None` for `single`
/// because §2.2 only allows manifest-as-sidecar with `input_type=sidecar`,
/// and encryption is restricted to `single` here (§2.4).
fn decrypt_single_payload(
    suite: EncryptionSuite,
    input: &InputData,
    fetched: &crate::content_fetch::FetchedContent,
    key_bundle: &KeyBundle,
) -> Result<
    (
        Vec<u8>,
        String,
        Option<Vec<u8>>,
        Option<ResponseChannel>,
        Option<String>,
    ),
    OrchestratorError,
> {
    let content_url = match input {
        InputData::Single { content_url } => content_url.as_str(),
        _ => return Err(OrchestratorError::EncryptionRequiresSingleInput),
    };

    let opened = open_request(key_bundle, suite, &fetched.content_bytes).map_err(|e| match e {
        title_crypto::CryptoError::EncryptionSuiteMismatch { wire, .. } => {
            OrchestratorError::EncryptionSuiteMismatch {
                declared: suite,
                wire: EncryptionSuite::from_suite_id(wire).unwrap_or(suite),
            }
        }
        _ => OrchestratorError::DecryptionFailed(format!("{e:?}")),
    })?;

    let parsed = payload::parse_payload(&opened.plaintext)
        .map_err(|e| OrchestratorError::PayloadMetadataInvalid(format!("{e:?}")))?;

    // The fetched bytes were the encrypted wire payload, so `fetched.content_type`
    // describes the ciphertext (typically `application/octet-stream`). Re-detect
    // the type from the decrypted content bytes so c2pa-verify and the rest of
    // the pipeline see the actual media type.
    let plaintext_content_type =
        detect_content_type(parsed.content, content_url, Some(&fetched.content_type));

    Ok((
        parsed.content.to_vec(),
        plaintext_content_type,
        None,
        Some(opened.response_channel),
        Some(parsed.metadata.signature_hash),
    ))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Ensure `c2pa-verify` is in the processor ID list.
/// Spec §1.3 -- c2pa-verify is mandatory for all requests.
///
/// If the client did not include `c2pa-verify` in `processor_ids`,
/// it is prepended to the list.
fn ensure_c2pa_verify(processor_ids: &[String]) -> Vec<String> {
    let mut ids = processor_ids.to_vec();
    if !ids.iter().any(|id| id == C2PA_VERIFY_PROCESSOR_ID) {
        ids.insert(0, C2PA_VERIFY_PROCESSOR_ID.to_string());
    }
    ids
}

/// Execute processors and collect results.
/// Spec §3.1 -- each processor runs independently.
fn execute_processors(
    registry: &ProcessorRegistry,
    processor_ids: &[String],
    content: &[u8],
    content_type: &str,
) -> HashMap<String, ProcessorOutput> {
    registry.execute(processor_ids, content, content_type)
}

/// Compute JCS-canonicalized SHA-256 hash of a VerifiableResponse.
/// Spec §1.5, §2.3
///
/// The hash is used as `user_data` in the Attestation Document, binding
/// the processing results to the TEE attestation.
fn compute_jcs_hash(verifiable: &VerifiableResponse) -> Result<Vec<u8>, OrchestratorError> {
    let json_value = serde_json::to_value(verifiable)?;
    let jcs_bytes = serde_json_canonicalizer::to_vec(&json_value)
        .map_err(|e| OrchestratorError::JcsFailed(e.to_string()))?;
    let hash = Sha256::digest(&jcs_bytes);
    Ok(hash.to_vec())
}

/// Build the final ProcessResponse with Attestation Document.
/// Spec §1.2, §2.3
///
/// 1. JCS-canonicalize the VerifiableResponse
/// 2. SHA-256 hash
/// 3. Get Attestation Document with hash as user_data
/// 4. Base64-encode the Attestation Document
/// 5. Assemble ProcessResponse
fn build_attested_response(
    verifiable: VerifiableResponse,
    runtime: &dyn TeeRuntime,
) -> Result<ProcessResponse, OrchestratorError> {
    // Step 7: JCS canonicalize and hash
    let jcs_hash = compute_jcs_hash(&verifiable)?;

    // Step 8: Get Attestation Document
    // Spec §1.2 -- user_data = SHA-256(JCS(verifiable))
    let attestation_doc = runtime
        .get_attestation_document(&jcs_hash)
        .map_err(|e| OrchestratorError::AttestationFailed(e.to_string()))?;

    // Step 9: Base64-encode Attestation Document
    let attestation = base64::engine::general_purpose::STANDARD.encode(&attestation_doc);

    Ok(ProcessResponse {
        verifiable,
        attestation,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_fetch::{ContentFetcher, FetchError, FetchResponse};
    use crate::resource_pool::ResourcePool;
    use crate::TeeError;
    use std::sync::Mutex;
    use title_core::{C2paVerifyProcessor, ProcessorRegistry};

    // ---- Mock ContentFetcher ----

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
                etag: Some("\"mock-etag\"".to_string()),
            })
        }
    }

    // ---- Mock TeeRuntime ----

    struct MockRuntime {
        received_user_data: Mutex<Option<Vec<u8>>>,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                received_user_data: Mutex::new(None),
            }
        }

        /// Returns the user_data that was passed to get_attestation_document.
        fn last_user_data(&self) -> Option<Vec<u8>> {
            self.received_user_data.lock().unwrap().clone()
        }
    }

    impl TeeRuntime for MockRuntime {
        fn tee_type(&self) -> &str {
            "mock"
        }

        fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError> {
            *self.received_user_data.lock().unwrap() = Some(user_data.to_vec());
            // Return a mock attestation: prefix + user_data for testability
            let mut doc = b"mock-attestation:".to_vec();
            doc.extend_from_slice(user_data);
            Ok(doc)
        }

        fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError> {
            Ok(vec![0u8; len])
        }
    }

    // ---- Test helpers ----

    /// Creates a minimal valid JPEG (4x4 pixels).
    fn create_test_jpeg() -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, Rgb};
        use std::io::Cursor;

        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(4, 4, |x, y| Rgb([(x * 60) as u8, (y * 60) as u8, 128]));

        let mut buf = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut buf)
            .write_image(img.as_raw(), 4, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        buf.into_inner()
    }

    /// Creates a C2PA-signed JPEG for testing.
    fn create_signed_jpeg() -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer = c2pa::EphemeralSigner::new("title-orchestrator-test")
            .expect("Failed to create EphemeralSigner");

        let definition = serde_json::json!({
            "claim_generator_info": [{
                "name": "title-orchestrator-test",
                "version": "0.1.0"
            }],
            "assertions": [{
                "label": "c2pa.actions.v2",
                "data": {
                    "actions": [{
                        "action": "c2pa.created",
                        "digitalSourceType":
                            "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"
                    }]
                }
            }]
        });

        let mut source = std::io::Cursor::new(&test_jpeg);
        let mut output = std::io::Cursor::new(Vec::new());

        c2pa::Builder::from_context(c2pa::Context::default())
            .with_definition(definition.to_string())
            .expect("Builder definition failed")
            .sign(&signer, "image/jpeg", &mut source, &mut output)
            .expect("Signing failed");

        output.into_inner()
    }

    /// Sets up a registry with the real C2paVerifyProcessor.
    fn create_registry() -> ProcessorRegistry {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(C2paVerifyProcessor::new()));
        registry
    }

    /// Creates a large-enough pool for tests that don't care about limits.
    fn test_pool() -> Arc<ResourcePool> {
        Arc::new(ResourcePool::with_single_limit(1_000_000_000)) // 1 GB
    }

    /// Fresh KeyBundle for tests. The encryption path isn't exercised in the
    /// plaintext-pipeline tests, but the orchestrator now requires one.
    fn test_key_bundle() -> KeyBundle {
        KeyBundle::generate(&mut rand::rngs::OsRng).expect("KeyBundle gen")
    }

    /// Convenience: unwrap a `ProcessOutcome::Plaintext` or panic.
    fn unwrap_plaintext(outcome: ProcessOutcome) -> ProcessResponse {
        match outcome {
            ProcessOutcome::Plaintext(r) => r,
            ProcessOutcome::Encrypted(_) => panic!("expected plaintext outcome"),
        }
    }

    // ---- ensure_c2pa_verify ----

    #[test]
    fn c2pa_verify_added_when_missing() {
        let ids = ensure_c2pa_verify(&["image-pdq".into()]);
        assert_eq!(ids[0], "c2pa-verify");
        assert_eq!(ids[1], "image-pdq");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn c2pa_verify_not_duplicated_when_present() {
        let ids = ensure_c2pa_verify(&["c2pa-verify".into(), "image-pdq".into()]);
        assert_eq!(ids.len(), 2);
        assert_eq!(
            ids.iter().filter(|id| id.as_str() == "c2pa-verify").count(),
            1
        );
    }

    #[test]
    fn c2pa_verify_added_to_empty_list() {
        let ids = ensure_c2pa_verify(&[]);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "c2pa-verify");
    }

    // ---- compute_jcs_hash ----

    #[test]
    fn jcs_hash_deterministic() {
        let verifiable = VerifiableResponse {
            signature_hash: "sha256:abcdef1234".into(),
            results: {
                let mut m = HashMap::new();
                m.insert(
                    "c2pa-verify".into(),
                    ProcessorOutput::ok(serde_json::json!({"validation": "valid"})),
                );
                m
            },
        };

        let hash1 = compute_jcs_hash(&verifiable).unwrap();
        let hash2 = compute_jcs_hash(&verifiable).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn jcs_hash_changes_with_different_data() {
        let v1 = VerifiableResponse {
            signature_hash: "sha256:aaaa".into(),
            results: HashMap::new(),
        };
        let v2 = VerifiableResponse {
            signature_hash: "sha256:bbbb".into(),
            results: HashMap::new(),
        };

        let hash1 = compute_jcs_hash(&v1).unwrap();
        let hash2 = compute_jcs_hash(&v2).unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn jcs_hash_is_sha256_length() {
        let v = VerifiableResponse {
            signature_hash: "sha256:test".into(),
            results: HashMap::new(),
        };
        let hash = compute_jcs_hash(&v).unwrap();
        assert_eq!(hash.len(), 32); // SHA-256 = 32 bytes
    }

    // ---- Full pipeline tests ----

    #[test]
    fn pipeline_single_content_success() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        let response = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &runtime,
                &pool,
                &test_key_bundle(),
            )
            .unwrap(),
        );

        // Verify signature_hash is present and has correct format
        assert!(response.verifiable.signature_hash.starts_with("sha256:"));

        // Verify c2pa-verify result exists and is OK
        let c2pa_result = &response.verifiable.results["c2pa-verify"];
        assert_eq!(c2pa_result.status, title_core::ProcessorStatus::Ok);

        // Verify attestation is present (Base64-encoded mock attestation)
        assert!(!response.attestation.is_empty());

        // Verify attestation contains the JCS hash
        let attestation_bytes = base64::engine::general_purpose::STANDARD
            .decode(&response.attestation)
            .unwrap();
        assert!(attestation_bytes.starts_with(b"mock-attestation:"));

        // The JCS hash in the attestation should match a recomputation
        let expected_hash = compute_jcs_hash(&response.verifiable).unwrap();
        assert_eq!(runtime.last_user_data().unwrap(), expected_hash);

        // Pool should be empty after response (Ticket dropped)
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn pipeline_c2pa_verify_implicit_addition() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        // Request does NOT include c2pa-verify in processor_ids
        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec![], // Empty -- c2pa-verify should be auto-added
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        let response = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &runtime,
                &pool,
                &test_key_bundle(),
            )
            .unwrap(),
        );

        // c2pa-verify should be in results even though not requested
        assert!(
            response.verifiable.results.contains_key("c2pa-verify"),
            "c2pa-verify should be implicitly added"
        );
        assert_eq!(
            response.verifiable.results["c2pa-verify"].status,
            title_core::ProcessorStatus::Ok
        );
    }

    #[test]
    fn pipeline_unsigned_content_rejected() {
        let unsigned_jpeg = create_test_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/unsigned.jpg",
            unsigned_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/unsigned.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        // Unsigned content should fail at signature_hash computation
        let result = process_request(
            &request,
            &fetcher,
            &registry,
            &runtime,
            &pool,
            &test_key_bundle(),
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            OrchestratorError::SignatureHashFailed(_)
        ));
    }

    #[test]
    fn pipeline_fetch_failure_propagated() {
        let fetcher = MockFetcher::new(); // No responses configured

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/missing.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        let result = process_request(
            &request,
            &fetcher,
            &registry,
            &runtime,
            &pool,
            &test_key_bundle(),
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            OrchestratorError::FetchFailed(_)
        ));
    }

    #[test]
    fn pipeline_unknown_processor_error_in_results() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into(), "nonexistent-proc".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        // Pipeline should succeed even though one processor is unknown
        let response = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &runtime,
                &pool,
                &test_key_bundle(),
            )
            .unwrap(),
        );

        // c2pa-verify should succeed
        assert_eq!(
            response.verifiable.results["c2pa-verify"].status,
            title_core::ProcessorStatus::Ok
        );
        // Unknown processor should have error status
        assert_eq!(
            response.verifiable.results["nonexistent-proc"].status,
            title_core::ProcessorStatus::Error
        );
    }

    #[test]
    fn attestation_binds_jcs_hash() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        let response = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &runtime,
                &pool,
                &test_key_bundle(),
            )
            .unwrap(),
        );

        // Recompute JCS hash from the response's verifiable part
        let recomputed_hash = compute_jcs_hash(&response.verifiable).unwrap();

        // The mock runtime should have received this exact hash
        let attestation_user_data = runtime.last_user_data().unwrap();
        assert_eq!(
            attestation_user_data, recomputed_hash,
            "Attestation user_data must match JCS hash of VerifiableResponse"
        );

        // Verify the attestation decodes and contains the hash
        let attestation_bytes = base64::engine::general_purpose::STANDARD
            .decode(&response.attestation)
            .unwrap();
        let expected_prefix = b"mock-attestation:";
        assert_eq!(&attestation_bytes[..expected_prefix.len()], expected_prefix);
        assert_eq!(
            &attestation_bytes[expected_prefix.len()..],
            &recomputed_hash[..]
        );
    }

    #[test]
    fn signature_hash_deterministic_across_requests() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();

        let response1 = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &MockRuntime::new(),
                &test_pool(),
                &test_key_bundle(),
            )
            .unwrap(),
        );
        let response2 = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &MockRuntime::new(),
                &test_pool(),
                &test_key_bundle(),
            )
            .unwrap(),
        );

        assert_eq!(
            response1.verifiable.signature_hash, response2.verifiable.signature_hash,
            "Same content must produce the same signature_hash"
        );
    }

    #[test]
    fn response_serialization_matches_spec() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();
        let response = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &runtime,
                &pool,
                &test_key_bundle(),
            )
            .unwrap(),
        );

        // Serialize and verify structure matches spec §2.3
        let json = serde_json::to_value(&response).unwrap();

        // Top-level fields (flatten from VerifiableResponse)
        assert!(json["signature_hash"].is_string());
        assert!(json["signature_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(json["results"].is_object());
        assert!(json["attestation"].is_string());

        // c2pa-verify result
        let c2pa = &json["results"]["c2pa-verify"];
        assert_eq!(c2pa["status"], "ok");
        assert!(c2pa["validation"].is_string());
    }

    // ---- Admission control ----

    #[test]
    fn pipeline_admission_rejected_when_pool_full() {
        let signed_jpeg = create_signed_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();

        // Pool with admission_limit=0 -- rejects all new requests
        let pool = Arc::new(ResourcePool::new(0, 1000));

        let result = process_request(
            &request,
            &fetcher,
            &registry,
            &runtime,
            &pool,
            &test_key_bundle(),
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            OrchestratorError::AdmissionRejected
        ));
    }

    #[test]
    fn pipeline_memory_released_after_completion() {
        let signed_jpeg = create_signed_jpeg();
        let jpeg_len = signed_jpeg.len();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            signed_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/photo.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        // Pool just big enough for the JPEG
        let pool = Arc::new(ResourcePool::with_single_limit(jpeg_len + 1024));

        // Before: pool is empty
        assert_eq!(pool.total_used(), 0);

        let _response = unwrap_plaintext(
            process_request(
                &request,
                &fetcher,
                &registry,
                &runtime,
                &pool,
                &test_key_bundle(),
            )
            .unwrap(),
        );

        // After: Ticket dropped, pool should be empty again
        assert_eq!(
            pool.total_used(),
            0,
            "Memory must be released after request completes"
        );
    }

    #[test]
    fn pipeline_memory_released_on_error() {
        let unsigned_jpeg = create_test_jpeg();
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/unsigned.jpg",
            unsigned_jpeg,
            Some("image/jpeg"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/unsigned.jpg".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };

        let registry = create_registry();
        let runtime = MockRuntime::new();
        let pool = test_pool();

        // This will fail at signature_hash (unsigned content)
        let result = process_request(
            &request,
            &fetcher,
            &registry,
            &runtime,
            &pool,
            &test_key_bundle(),
        );
        assert!(result.is_err());

        // Memory should still be released despite the error
        assert_eq!(
            pool.total_used(),
            0,
            "Memory must be released even on error"
        );
    }

    // ---- Encryption pipeline (§2.4) ----

    /// End-to-end roundtrip: client encrypts a C2PA-signed file using the
    /// TEE's published public key, uploads the wire payload, the TEE decrypts
    /// it, runs c2pa-verify, encrypts the response, and the client decrypts
    /// and verifies the response binds to the same content.
    #[test]
    fn encrypted_pipeline_x25519_roundtrip() {
        use title_core::{EncryptedPayloadMetadata, EncryptionSuite};
        use title_crypto::{payload as crypto_payload, sealed_channel};

        // 1. Server side: bring up a TEE key bundle + mock fetcher.
        let key_bundle = test_key_bundle();
        let tee_pub = key_bundle.public_key_bytes(EncryptionSuite::X25519);
        let signed = create_signed_jpeg();
        let signature_hash =
            compute_signature_hash(&signed, "image/jpeg").expect("signed content has sig");

        // 2. Client side: build the inner plaintext payload exactly as Spec §2.4
        //    prescribes, then encrypt for the TEE.
        let metadata = EncryptedPayloadMetadata {
            signature_hash: signature_hash.clone(),
        };
        let inner_payload = crypto_payload::build_payload(&metadata, &signed);
        let (wire, response_channel) =
            sealed_channel::seal_for(EncryptionSuite::X25519, &tee_pub, &inner_payload)
                .expect("seal_for");

        // 3. Mock the storage endpoint to return the encrypted wire payload.
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/encrypted.bin",
            wire,
            Some("application/octet-stream"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/encrypted.bin".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: Some(EncryptionSuite::X25519),
        };

        // 4. Run the TEE pipeline.
        let outcome = process_request(
            &request,
            &fetcher,
            &create_registry(),
            &MockRuntime::new(),
            &test_pool(),
            &key_bundle,
        )
        .expect("process_request");

        // 5. The outcome must be sealed, decryptable only by the client's
        //    response_channel.
        let ciphertext = match outcome {
            ProcessOutcome::Encrypted(bytes) => bytes,
            ProcessOutcome::Plaintext(_) => panic!("encrypted request must return Encrypted"),
        };
        let plaintext_response = response_channel.open(&ciphertext).expect("open response");

        let response: ProcessResponse =
            serde_json::from_slice(&plaintext_response).expect("response is JSON");
        assert_eq!(response.verifiable.signature_hash, signature_hash);
        let c2pa_result = &response.verifiable.results["c2pa-verify"];
        assert_eq!(c2pa_result.status, title_core::ProcessorStatus::Ok);
    }

    /// If the client lies about `signature_hash` in the encrypted payload's
    /// metadata, the TEE must reject (§2.4 step 8).
    #[test]
    fn encrypted_pipeline_signature_hash_mismatch_rejected() {
        use title_core::{EncryptedPayloadMetadata, EncryptionSuite};
        use title_crypto::{payload as crypto_payload, sealed_channel};

        let key_bundle = test_key_bundle();
        let tee_pub = key_bundle.public_key_bytes(EncryptionSuite::X25519);
        let signed = create_signed_jpeg();

        // Lie: declare a different signature_hash than the content actually has.
        let lying_metadata = EncryptedPayloadMetadata {
            signature_hash: "sha256:deadbeef".into(),
        };
        let inner = crypto_payload::build_payload(&lying_metadata, &signed);
        let (wire, _channel) =
            sealed_channel::seal_for(EncryptionSuite::X25519, &tee_pub, &inner).unwrap();

        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://storage.example.com/encrypted.bin",
            wire,
            Some("application/octet-stream"),
        );

        let request = ProcessRequest {
            input: title_core::InputData::Single {
                content_url: "https://storage.example.com/encrypted.bin".to_string(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: Some(EncryptionSuite::X25519),
        };

        let err = process_request(
            &request,
            &fetcher,
            &create_registry(),
            &MockRuntime::new(),
            &test_pool(),
            &key_bundle,
        )
        .expect_err("mismatched declared signature_hash must error");

        assert!(matches!(err, OrchestratorError::SignatureHashMismatch));
    }

    /// `encryption` with fragmented/sidecar input is explicitly out of scope
    /// for this protocol version and must be rejected with a clear error.
    #[test]
    fn encrypted_pipeline_rejects_fragmented_input() {
        use title_core::EncryptionSuite;

        let request = ProcessRequest {
            input: title_core::InputData::Fragmented {
                init_url: "https://example.com/init.mp4".into(),
                fragment_urls: vec!["https://example.com/seg-0.m4s".into()],
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: Some(EncryptionSuite::X25519),
        };

        // Empty fetcher is fine — we should bail before any fetch attempt
        // for unsupported input types. Actually fetcher gets called first;
        // give it a stub response so the test reaches the encryption gate.
        let mut fetcher = MockFetcher::new();
        fetcher.add(
            "https://example.com/init.mp4",
            vec![0u8; 16],
            Some("video/mp4"),
        );
        fetcher.add(
            "https://example.com/seg-0.m4s",
            vec![0u8; 16],
            Some("video/iso.segment"),
        );

        let err = process_request(
            &request,
            &fetcher,
            &create_registry(),
            &MockRuntime::new(),
            &test_pool(),
            &test_key_bundle(),
        )
        .expect_err("fragmented + encryption must error");

        assert!(matches!(
            err,
            OrchestratorError::EncryptionRequiresSingleInput
        ));
    }
}
