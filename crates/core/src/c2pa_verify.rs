// SPDX-License-Identifier: Apache-2.0

//! # c2pa-verify Processor
//!
//! Spec §3.2 c2pa-verify
//!
//! Validates the C2PA signature chain and extracts manifest information.
//! This processor is mandatory for all requests — it is always executed
//! regardless of whether the client includes it in `processor_ids`.
//!
//! ## Outputs
//! - `validation`: `"valid"` or `"invalid"` (spec §3.2)
//! - `signer`: issuer and cert serial from the signing certificate
//! - `timestamp`: C2PA signing timestamp (ISO 8601)
//! - `claim_generator`: signing tool/device name
//! - `actions`: action history from the manifest
//!
//! ## signature_hash utility
//!
//! `compute_signature_hash()` computes SHA-256 of the Active Manifest's
//! COSE signature bytes, formatted as `"sha256:hex..."`. This value
//! is used as the protocol-level content identifier (spec §1.3) and
//! appears in the top-level `ProcessResponse` (spec §2.3).
//!
//! The utility is public because the TEE orchestration layer (Task 04)
//! also needs it when assembling the final response.

use crate::jumbf;
use crate::processor::{Processor, ProcessorError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;

/// c2pa-verify processor output. Spec §3.2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct C2paVerifyOutput {
    /// `"valid"` or `"invalid"` — an invalid signature is still a successful
    /// processor run.
    pub validation: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerInfo>,

    /// C2PA signing timestamp in ISO 8601.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    /// Signing tool / device name. Built from `claim_generator_info` when
    /// the manifest carries it (preferred), else the raw `claim_generator`
    /// string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_generator: Option<String>,

    #[serde(default)]
    pub actions: Vec<C2paAction>,
}

/// Issuer and certificate serial of the C2PA signing identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerInfo {
    pub issuer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_serial: Option<String>,
}

/// One entry from the manifest's action history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct C2paAction {
    pub action: String,
}

/// c2pa-verify processor ID.
/// Spec §3.2
pub const C2PA_VERIFY_PROCESSOR_ID: &str = "c2pa-verify";

/// c2pa-verify processor.
/// Spec §3.2 c2pa-verify
///
/// Validates C2PA signature chains and extracts manifest metadata.
/// Always runs for every request — C2PA signature is the trust foundation
/// of Title Protocol.
pub struct C2paVerifyProcessor;

impl C2paVerifyProcessor {
    /// Creates a new c2pa-verify processor.
    pub fn new() -> Self {
        Self
    }
}

impl Default for C2paVerifyProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for C2paVerifyProcessor {
    /// Returns `"c2pa-verify"`.
    /// Spec §3.2
    fn id(&self) -> &str {
        C2PA_VERIFY_PROCESSOR_ID
    }

    /// Verifies the C2PA signature chain and extracts manifest attributes.
    /// Spec §3.2 c2pa-verify
    ///
    /// # Arguments
    /// * `content` — C2PA-signed content bytes (JPEG, PNG, MP4, etc.)
    /// * `content_type` — MIME type of the content
    ///
    /// # Returns
    /// `C2paVerifyOutput` serialized as `serde_json::Value`.
    ///
    /// If the C2PA signature is invalid, `validation` is `"invalid"` but
    /// the processor does NOT return an error — an invalid verification
    /// result is still a valid processor output (spec §3.2).
    ///
    /// Returns `ProcessorError` only when the content cannot be parsed
    /// at all (no C2PA data, corrupt file, etc.).
    fn process(
        &self,
        content: &[u8],
        content_type: &str,
    ) -> Result<serde_json::Value, ProcessorError> {
        let output = verify_and_extract(content, content_type)?;
        serde_json::to_value(&output).map_err(|e| {
            ProcessorError::Internal(format!("Failed to serialize C2paVerifyOutput: {e}"))
        })
    }
}

/// Computes the signature_hash for C2PA-signed content.
/// Spec §1.3 — signature_hash = SHA-256(Active Manifest's COSE signature)
///
/// Returns the hash in `"sha256:hex..."` format.
///
/// This function is used by:
/// - The TEE orchestration layer (Task 04) to populate `ProcessResponse.signature_hash`
/// - Verification of encrypted payloads (spec §2.4 step 8)
///
/// # Arguments
/// * `content` — C2PA-signed content bytes
/// * `content_type` — MIME type of the content
///
/// # Determinism
/// The same content always produces the same signature_hash, regardless
/// of who computes it. This property is essential for the protocol's
/// verification model (spec §1.5).
pub fn compute_signature_hash(
    content: &[u8],
    content_type: &str,
) -> Result<String, ProcessorError> {
    let signature_bytes = extract_active_manifest_signature(content, content_type)?;
    let hash = Sha256::digest(&signature_bytes);
    Ok(format!("sha256:{}", hex::encode(hash)))
}

/// Computes the signature_hash from raw JUMBF manifest data (for sidecar inputs).
/// Spec §1.3 — signature_hash for sidecar content.
///
/// The `.c2pa` sidecar file contains raw JUMBF data (the manifest store).
/// C2PA 2.1 §13.4 defines the active manifest as the last `c2pa.manifest`
/// box in the store; this function follows that ordering convention,
/// extracts the COSE signature from the active manifest, and computes
/// SHA-256.
///
/// # Arguments
/// * `manifest_data` — Raw JUMBF bytes from a `.c2pa` sidecar file
pub fn compute_signature_hash_from_manifest_data(
    manifest_data: &[u8],
) -> Result<String, ProcessorError> {
    let labels = jumbf::find_manifest_labels(manifest_data)?;
    let active_label = labels.last().ok_or_else(|| {
        ProcessorError::C2paVerificationFailed(
            "No manifest found in JUMBF sidecar data".to_string(),
        )
    })?;

    let signature_bytes =
        jumbf::extract_signature_from_jumbf(manifest_data, active_label)?;
    let hash = Sha256::digest(&signature_bytes);
    Ok(format!("sha256:{}", hex::encode(hash)))
}

// ---------------------------------------------------------------------------
// Internal implementation
// ---------------------------------------------------------------------------

/// Extracts the Active Manifest's COSE signature bytes from content.
/// Spec §1.3
///
/// Uses `c2pa::jumbf_io::load_jumbf_from_memory` to extract raw JUMBF data,
/// then the JUMBF parser to locate the c2pa.signature box and extract CBOR.
fn extract_active_manifest_signature(
    content: &[u8],
    content_type: &str,
) -> Result<Vec<u8>, ProcessorError> {
    // Parse with c2pa::Reader to get the active manifest label
    let mut cursor = Cursor::new(content);
    let reader = c2pa::Reader::from_context(c2pa::Context::default())
        .with_stream(content_type, &mut cursor)
        .map_err(|e| {
        ProcessorError::C2paVerificationFailed(format!("C2PA Reader construction failed: {e}"))
    })?;

    let active_label = reader
        .active_label()
        .ok_or_else(|| {
            ProcessorError::C2paVerificationFailed(
                "Active Manifest not found".to_string(),
            )
        })?
        .to_string();

    // Extract raw JUMBF data from content
    let jumbf_data =
        c2pa::jumbf_io::load_jumbf_from_memory(content_type, content).map_err(|e| {
            ProcessorError::C2paVerificationFailed(format!("JUMBF extraction failed: {e}"))
        })?;

    // Extract COSE signature bytes from JUMBF
    jumbf::extract_signature_from_jumbf(&jumbf_data, &active_label)
}

/// Verifies C2PA and extracts metadata into `C2paVerifyOutput`.
/// Spec §3.2 c2pa-verify
fn verify_and_extract(
    content: &[u8],
    content_type: &str,
) -> Result<C2paVerifyOutput, ProcessorError> {
    let mut cursor = Cursor::new(content);
    let reader = c2pa::Reader::from_context(c2pa::Context::default())
        .with_stream(content_type, &mut cursor)
        .map_err(|e| {
        ProcessorError::C2paVerificationFailed(format!("C2PA Reader construction failed: {e}"))
    })?;

    // Determine validation state
    // Spec §3.2: "valid" when signature chain is structurally valid (Valid or Trusted)
    let validation_state = reader.validation_state();
    let validation = match validation_state {
        c2pa::validation_results::ValidationState::Valid
        | c2pa::validation_results::ValidationState::Trusted => "valid",
        c2pa::validation_results::ValidationState::Invalid => "invalid",
    };

    let manifest = reader.active_manifest().ok_or_else(|| {
        ProcessorError::C2paVerificationFailed("Active Manifest not found".to_string())
    })?;

    // Extract signer info from SignatureInfo
    // Spec §3.2: signer.issuer + signer.cert_serial
    let signer = manifest.signature_info().map(|sig_info| SignerInfo {
        issuer: sig_info
            .issuer
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        cert_serial: sig_info.cert_serial_number.clone(),
    });

    // Extract timestamp
    // Spec §3.2: ISO 8601 format
    let timestamp = manifest.time();

    // Extract claim_generator
    // Spec §3.2: signing tool/device name
    // Use claim_generator_info (name + version) if available, fall back to claim_generator
    let claim_generator = build_claim_generator_string(manifest);

    // Extract actions
    // Spec §3.2: action history from manifest assertions
    let actions = extract_actions(manifest);

    Ok(C2paVerifyOutput {
        validation: validation.to_string(),
        signer,
        timestamp,
        claim_generator,
        actions,
    })
}

/// Builds the claim_generator display string.
///
/// Prefers `claim_generator_info` (structured data with name + version)
/// over the raw `claim_generator` string. If `claim_generator_info` has
/// entries, formats as "name version" for the first entry.
fn build_claim_generator_string(manifest: &c2pa::Manifest) -> Option<String> {
    // Try claim_generator_info first (structured, preferred)
    if let Some(info_vec) = &manifest.claim_generator_info {
        if let Some(info) = info_vec.first() {
            let name = &info.name;
            return Some(match &info.version {
                Some(ver) => format!("{name} {ver}"),
                None => name.clone(),
            });
        }
    }

    // Fall back to raw claim_generator string
    manifest.claim_generator().map(|s| s.to_string())
}

/// Extracts C2PA actions from manifest assertions.
/// Spec §3.2: action history
///
/// Looks for both `c2pa.actions` and `c2pa.actions.v2` assertion labels.
fn extract_actions(manifest: &c2pa::Manifest) -> Vec<C2paAction> {
    // Try c2pa.actions.v2 first (preferred), then c2pa.actions
    let actions_result: Option<c2pa::assertions::Actions> = manifest
        .find_assertion(c2pa::assertions::Actions::LABEL_VERSIONED)
        .ok()
        .or_else(|| manifest.find_assertion(c2pa::assertions::Actions::LABEL).ok());

    match actions_result {
        Some(actions) => actions
            .actions
            .iter()
            .map(|a| C2paAction {
                action: a.action().to_string(),
            })
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a minimal valid JPEG for testing.
    /// Uses the `image` crate to generate a 4x4 pixel JPEG image.
    fn create_test_jpeg() -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, Rgb};
        use std::io::Cursor;

        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |x, y| {
            Rgb([(x * 60) as u8, (y * 60) as u8, 128])
        });

        let mut buf = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut buf)
            .write_image(img.as_raw(), 4, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        buf.into_inner()
    }

    /// Creates a C2PA-signed JPEG for testing.
    /// Uses EphemeralSigner (self-signed test certificate).
    fn create_signed_jpeg() -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer =
            c2pa::EphemeralSigner::new("title-core-test").expect("EphemeralSigner creation");

        let definition = serde_json::json!({
            "claim_generator_info": [{
                "name": "title-core-test",
                "version": "0.1.2"
            }],
            "assertions": [{
                "label": "c2pa.actions.v2",
                "data": {
                    "actions": [{
                        "action": "c2pa.created"
                    }]
                }
            }]
        });

        let mut builder = c2pa::Builder::from_context(c2pa::Context::default())
            .with_definition(&definition.to_string())
            .expect("Builder creation");

        let mut source = Cursor::new(&test_jpeg);
        let mut dest = Cursor::new(Vec::new());
        builder
            .sign(&signer, "image/jpeg", &mut source, &mut dest)
            .expect("C2PA signing");
        dest.into_inner()
    }

    /// Creates a C2PA-signed JPEG with multiple actions.
    fn create_signed_jpeg_with_actions(actions: &[&str]) -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer =
            c2pa::EphemeralSigner::new("title-core-test").expect("EphemeralSigner creation");

        let actions_json: Vec<serde_json::Value> = actions
            .iter()
            .map(|a| serde_json::json!({ "action": a }))
            .collect();

        let definition = serde_json::json!({
            "claim_generator_info": [{
                "name": "title-core-test",
                "version": "0.1.2"
            }],
            "assertions": [{
                "label": "c2pa.actions.v2",
                "data": {
                    "actions": actions_json
                }
            }]
        });

        let mut builder = c2pa::Builder::from_context(c2pa::Context::default())
            .with_definition(&definition.to_string())
            .expect("Builder creation");

        let mut source = Cursor::new(&test_jpeg);
        let mut dest = Cursor::new(Vec::new());
        builder
            .sign(&signer, "image/jpeg", &mut source, &mut dest)
            .expect("C2PA signing");
        dest.into_inner()
    }

    // ── Processor trait tests ──

    #[test]
    fn processor_id() {
        let proc = C2paVerifyProcessor::new();
        assert_eq!(proc.id(), "c2pa-verify");
    }

    #[test]
    fn process_signed_content() {
        let signed = create_signed_jpeg();
        let proc = C2paVerifyProcessor::new();
        let result = proc.process(&signed, "image/jpeg");
        assert!(result.is_ok(), "process() should succeed: {result:?}");

        let value = result.unwrap();
        let output: C2paVerifyOutput =
            serde_json::from_value(value).expect("Should deserialize to C2paVerifyOutput");

        // Self-signed cert → ValidationState may be Invalid (untrusted) or Valid
        // depending on c2pa-rs version. The key assertion is that it doesn't error.
        assert!(
            output.validation == "valid" || output.validation == "invalid",
            "validation should be 'valid' or 'invalid', got: {}",
            output.validation
        );

        // Signer info should be extracted
        assert!(output.signer.is_some(), "signer should be present");

        // Actions should be extracted
        assert_eq!(output.actions.len(), 1);
        assert_eq!(output.actions[0].action, "c2pa.created");

        // claim_generator should be extracted
        assert!(
            output.claim_generator.is_some(),
            "claim_generator should be present"
        );
        let cg = output.claim_generator.unwrap();
        assert!(
            cg.contains("title-core-test"),
            "claim_generator should contain our test name, got: {cg}"
        );
    }

    #[test]
    fn process_unsigned_content_returns_error() {
        let unsigned = create_test_jpeg();
        let proc = C2paVerifyProcessor::new();
        let result = proc.process(&unsigned, "image/jpeg");
        assert!(result.is_err(), "unsigned content should return error");
        match result {
            Err(ProcessorError::C2paVerificationFailed(_)) => {} // expected
            other => panic!("Expected C2paVerificationFailed, got: {other:?}"),
        }
    }

    #[test]
    fn process_empty_content_returns_error() {
        let proc = C2paVerifyProcessor::new();
        let result = proc.process(&[], "image/jpeg");
        assert!(result.is_err(), "empty content should return error");
    }

    #[test]
    fn process_garbage_content_returns_error() {
        let proc = C2paVerifyProcessor::new();
        let result = proc.process(b"not a valid image", "image/jpeg");
        assert!(result.is_err(), "garbage content should return error");
    }

    // ── signature_hash tests ──

    #[test]
    fn signature_hash_deterministic() {
        let signed = create_signed_jpeg();

        let hash1 = compute_signature_hash(&signed, "image/jpeg")
            .expect("signature_hash computation");
        let hash2 = compute_signature_hash(&signed, "image/jpeg")
            .expect("signature_hash computation");

        // Same content → same hash (deterministic)
        assert_eq!(hash1, hash2);

        // Format check: "sha256:hexstring"
        assert!(
            hash1.starts_with("sha256:"),
            "signature_hash should start with 'sha256:', got: {hash1}"
        );

        // SHA-256 hex = 64 characters
        let hex_part = &hash1["sha256:".len()..];
        assert_eq!(
            hex_part.len(),
            64,
            "hex part should be 64 chars, got: {}",
            hex_part.len()
        );

        // All hex characters
        assert!(
            hex_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hex part should be hex digits: {hex_part}"
        );
    }

    #[test]
    fn signature_hash_differs_for_different_content() {
        // Two separately signed copies of the same image have different signatures
        let signed1 = create_signed_jpeg();
        let signed2 = create_signed_jpeg();

        let hash1 = compute_signature_hash(&signed1, "image/jpeg").unwrap();
        let hash2 = compute_signature_hash(&signed2, "image/jpeg").unwrap();

        // Different signing events → different COSE signatures → different hashes
        assert_ne!(
            hash1, hash2,
            "Different signing events should produce different signature_hashes"
        );
    }

    #[test]
    fn signature_hash_unsigned_content_error() {
        let unsigned = create_test_jpeg();
        let result = compute_signature_hash(&unsigned, "image/jpeg");
        assert!(
            result.is_err(),
            "unsigned content should not produce signature_hash"
        );
    }

    // ── C2paVerifyOutput JSON compatibility (§3.2) ──

    #[test]
    fn output_matches_spec_json_structure() {
        let signed = create_signed_jpeg();
        let proc = C2paVerifyProcessor::new();
        let value = proc.process(&signed, "image/jpeg").unwrap();

        // Check that all expected fields are present
        assert!(value.get("validation").is_some(), "validation field missing");
        assert!(value.get("actions").is_some(), "actions field missing");

        // status field should NOT be present (it's added by ProcessorOutput::ok())
        assert!(
            value.get("status").is_none(),
            "status should not be in processor output"
        );
    }

    #[test]
    fn output_roundtrip_through_c2pa_verify_output() {
        let signed = create_signed_jpeg();
        let proc = C2paVerifyProcessor::new();
        let value = proc.process(&signed, "image/jpeg").unwrap();

        // Deserialize back to C2paVerifyOutput
        let output: C2paVerifyOutput = serde_json::from_value(value.clone()).unwrap();

        // Re-serialize and check round-trip
        let re_serialized = serde_json::to_value(&output).unwrap();
        assert_eq!(value, re_serialized, "round-trip should be lossless");
    }

    // ── Multiple actions ──

    #[test]
    fn extract_multiple_actions() {
        let signed =
            create_signed_jpeg_with_actions(&["c2pa.created", "c2pa.edited", "c2pa.cropped"]);
        let proc = C2paVerifyProcessor::new();
        let value = proc.process(&signed, "image/jpeg").unwrap();
        let output: C2paVerifyOutput = serde_json::from_value(value).unwrap();

        assert_eq!(output.actions.len(), 3);
        assert_eq!(output.actions[0].action, "c2pa.created");
        assert_eq!(output.actions[1].action, "c2pa.edited");
        assert_eq!(output.actions[2].action, "c2pa.cropped");
    }

    // ── ProcessorRegistry integration ──

    #[test]
    fn register_in_processor_registry() {
        let mut registry = crate::ProcessorRegistry::new();
        registry.register(Box::new(C2paVerifyProcessor::new()));

        assert!(registry.processor_ids().contains(&"c2pa-verify"));
        assert!(registry.get("c2pa-verify").is_some());
    }

    #[test]
    fn execute_via_registry() {
        let signed = create_signed_jpeg();
        let mut registry = crate::ProcessorRegistry::new();
        registry.register(Box::new(C2paVerifyProcessor::new()));

        let results = registry.execute(&["c2pa-verify".into()], &signed, "image/jpeg");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results["c2pa-verify"].status,
            crate::response::ProcessorStatus::Ok
        );
    }

    #[test]
    fn execute_via_registry_unsigned_content() {
        let unsigned = create_test_jpeg();
        let mut registry = crate::ProcessorRegistry::new();
        registry.register(Box::new(C2paVerifyProcessor::new()));

        let results =
            registry.execute(&["c2pa-verify".into()], &unsigned, "image/jpeg");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results["c2pa-verify"].status,
            crate::response::ProcessorStatus::Error,
            "unsigned content should produce error status"
        );
    }
}
