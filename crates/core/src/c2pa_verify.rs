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
//! The utility is public because the TEE orchestration layer also
//! needs it when assembling the final response.

use crate::content_stream::ContentStream;
use crate::jumbf;
use crate::processor::{Processor, ProcessorError};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};

/// c2pa-verify processor output is the C2PA manifest store itself.
/// Spec §3.2.
///
/// The processor returns the full manifest store as returned by
/// `c2pa::Reader::json()`, parsed into a [`serde_json::Value`]. Validity is
/// signalled by **presence**: a successful processor run means the manifest
/// validated (validation_state was `Valid` or `Trusted`); an invalid manifest
/// causes the processor to return [`ProcessorError`] and no manifest data is
/// recorded.
///
/// Consumers are expected to parse the result with a C2PA-aware parser. The
/// schema is whatever the c2pa-rs version in use emits from `Reader::json()` —
/// the C2PA standard ManifestStore layout with `active_manifest`, `manifests`,
/// `validation_results`, `validation_state`.
///
/// # Byte-determinism
///
/// The byte-level JSON output is **per-binary**, not per-content. `c2pa-rs`
/// can change object key order, optional-field inclusion, or numeric encoding
/// across minor versions; doing so changes the JCS hash of this output even
/// for identical input. The workspace pins `c2pa = "=0.84.1"` exactly for this
/// reason. A deliberate bump must coincide with a TEE rebuild and a new PCR0
/// registered on Solana — old attestations remain valid against their own
/// PCR0 / `c2pa` version pair, never against a newer one.
pub type C2paVerifyOutput = serde_json::Value;

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
    /// * `content` — C2PA-signed content stream (Read + Seek). c2pa-rs
    ///   only reads the box headers and the verification ranges, so a
    ///   Range-Request-backed reader works for large MP4 files without
    ///   loading the file into memory.
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
        content: &mut dyn ContentStream,
        content_type: &str,
    ) -> Result<serde_json::Value, ProcessorError> {
        let output = verify_and_extract(content, content_type)?;
        serde_json::to_value(&output).map_err(|e| {
            ProcessorError::Internal(format!("Failed to serialize C2paVerifyOutput: {e}"))
        })
    }
}

/// 仕様 §1.3 — `signature_hash = SHA-256(Active Manifest's COSE signature)`
/// を計算し `"sha256:hex..."` 形式で返す。`content` は C2PA 署名付き
/// コンテンツの `Read + Seek` stream、`content_type` は MIME タイプ。
///
/// 実装は `c2pa::jumbf_io::load_jumbf_from_stream` で JUMBF ボックスだけを
/// 抽出 (BMFF/JPEG/PNG いずれもファイル全体をメモリに載せずに seek で位置
/// 特定する)、その後 JUMBF 内の COSE 署名を取り出して SHA-256 を計算する。
/// 50 GB の MP4 でも Range Request reader 経由で JUMBF 周辺だけを読めば済む。
pub fn compute_signature_hash(
    content: &mut dyn ContentStream,
    content_type: &str,
) -> Result<String, ProcessorError> {
    let signature_bytes = extract_active_manifest_signature(content, content_type)?;
    Ok(format_signature_hash(&signature_bytes))
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

    let signature_bytes = jumbf::extract_signature_from_jumbf(manifest_data, active_label)?;
    Ok(format_signature_hash(&signature_bytes))
}

/// Format raw signature bytes into the spec §1.3 `signature_hash` string.
/// Single source of truth so a future digest swap touches one line.
fn format_signature_hash(signature_bytes: &[u8]) -> String {
    let hash = Sha256::digest(signature_bytes);
    format!("sha256:{}", hex::encode(hash))
}

// ---------------------------------------------------------------------------
// CAIRead 互換アダプタ
// ---------------------------------------------------------------------------

/// `c2pa::jumbf_io::load_jumbf_from_stream` は `&mut dyn CAIRead` を要求するが、
/// `CAIRead` は c2pa-rs 内部 trait で、`Read + Seek + MaybeSend` のブランケット
/// impl で公開される。`dyn ContentStream` から `dyn CAIRead` への trait object
/// 直接変換はできないため、`&mut dyn ContentStream` を保持する小さな具体型に
/// 包んで渡す。具体型は `Read + Seek + Send` を満たすので CAIRead が自動で付く。
struct CaiReadAdapter<'a> {
    inner: &'a mut dyn ContentStream,
}

impl<'a> Read for CaiReadAdapter<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<'a> Seek for CaiReadAdapter<'a> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

// ---------------------------------------------------------------------------
// Internal implementation
// ---------------------------------------------------------------------------

/// Extracts the Active Manifest's COSE signature bytes from content.
/// Spec §1.3
///
/// Uses `c2pa::jumbf_io::load_jumbf_from_stream` to pull the JUMBF box out
/// of a `Read + Seek` source (BMFF / JPEG / PNG にかかわらずヘッダだけを
/// 読む)、then the local JUMBF parser to locate the active manifest's
/// `c2pa.signature` box and return the COSE bytes.
///
/// JUMBF データそのものは数 KB なので [`compute_signature_hash_from_manifest_data`]
/// と同じ単一パス処理に合流させ、Reader の二重構築を避ける。
fn extract_active_manifest_signature(
    content: &mut dyn ContentStream,
    content_type: &str,
) -> Result<Vec<u8>, ProcessorError> {
    // 念のため stream を先頭へ巻き戻す (前段で seek されている場合がある)。
    content.seek(SeekFrom::Start(0)).map_err(|e| {
        ProcessorError::ReadFailed(format!("Failed to rewind content stream: {e}"))
    })?;

    // CAIRead trait object 互換のアダプタに包んで c2pa-rs に渡す。
    let mut adapter = CaiReadAdapter { inner: content };
    let jumbf_data = c2pa::jumbf_io::load_jumbf_from_stream(content_type, &mut adapter)
        .map_err(|e| {
            ProcessorError::C2paVerificationFailed(format!("JUMBF extraction failed: {e}"))
        })?;

    let labels = jumbf::find_manifest_labels(&jumbf_data)?;
    let active_label = labels.last().ok_or_else(|| {
        ProcessorError::C2paVerificationFailed("No manifest found in JUMBF data".to_string())
    })?;

    jumbf::extract_signature_from_jumbf(&jumbf_data, active_label)
}

/// Verifies the C2PA manifest and returns the full manifest store.
/// Spec §3.2 c2pa-verify
///
/// Returns the C2PA manifest store verbatim (`c2pa::Reader::json()`) as a
/// [`serde_json::Value`]. The output's *presence* is the validity signal:
/// the processor returns [`ProcessorError`] when the manifest cannot be
/// loaded, when no active manifest exists, or when the validation state is
/// `Invalid`. A successful return means the manifest validated `Valid` or
/// `Trusted` and the entire manifest store (including `validation_results`,
/// `validation_state`, `signature_info`, all assertions, all ingredients) is
/// preserved in the returned value for downstream re-verification without
/// the source content.
fn verify_and_extract(
    content: &mut dyn ContentStream,
    content_type: &str,
) -> Result<C2paVerifyOutput, ProcessorError> {
    // c2pa::Reader::with_stream は内部で box ヘッダだけを seek/read するので、
    // 巨大ファイルでも全部メモリに載せる必要はない。Range Request reader を
    // 直接ここに流せる。
    content.seek(SeekFrom::Start(0)).map_err(|e| {
        ProcessorError::ReadFailed(format!("Failed to rewind content stream: {e}"))
    })?;
    let reader = c2pa::Reader::from_context(c2pa::Context::default())
        .with_stream(content_type, &mut *content)
        .map_err(|e| {
            ProcessorError::C2paVerificationFailed(format!("C2PA Reader construction failed: {e}"))
        })?;

    // Require an active manifest. A reader without one is structurally
    // useless to downstream consumers.
    if reader.active_manifest().is_none() {
        return Err(ProcessorError::C2paVerificationFailed(
            "Active Manifest not found".to_string(),
        ));
    }

    // Reject `Invalid` validation. Presence of the output is the validity
    // signal — we never record an invalid manifest.
    match reader.validation_state() {
        c2pa::validation_results::ValidationState::Valid
        | c2pa::validation_results::ValidationState::Trusted => {}
        c2pa::validation_results::ValidationState::Invalid => {
            return Err(ProcessorError::C2paVerificationFailed(
                "C2PA validation failed (validation_state = Invalid)".to_string(),
            ));
        }
    }

    // `reader.json()` returns the entire ManifestStore as a JSON string in
    // the C2PA standard layout (active + ingredients, all assertions, all
    // signature info, all validation codes). Parsing it into a serde_json
    // Value lets us return it verbatim, so a verifier can reconstruct and
    // re-check the C2PA manifest without holding the source content.
    serde_json::from_str(&reader.json()).map_err(|e| {
        ProcessorError::Internal(format!("Failed to parse c2pa reader JSON: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_stream::{ContentSource, InMemorySource};
    use std::io::Cursor;

    /// テスト用ヘルパ: バイト列を InMemorySource にして processor に渡す。
    fn run_processor(
        processor: &C2paVerifyProcessor,
        bytes: &[u8],
        content_type: &str,
    ) -> Result<serde_json::Value, ProcessorError> {
        let source = InMemorySource::new(bytes.to_vec());
        let mut reader = source.open().expect("InMemorySource::open never fails");
        processor.process(reader.as_mut(), content_type)
    }

    /// テスト用ヘルパ: バイト列から signature_hash を計算。
    fn compute_signature_hash_bytes(
        bytes: &[u8],
        content_type: &str,
    ) -> Result<String, ProcessorError> {
        let source = InMemorySource::new(bytes.to_vec());
        let mut reader = source.open().expect("InMemorySource::open never fails");
        compute_signature_hash(reader.as_mut(), content_type)
    }

    /// Creates a minimal valid JPEG for testing.
    /// Uses the `image` crate to generate a 4x4 pixel JPEG image.
    fn create_test_jpeg() -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, Rgb};

        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(4, 4, |x, y| Rgb([(x * 60) as u8, (y * 60) as u8, 128]));

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
            .with_definition(definition.to_string())
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
            .with_definition(definition.to_string())
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
        // EphemeralSigner is expected to validate as Valid/Trusted under the
        // pinned c2pa-rs version. If this unwrap ever panics, the test trust
        // setup needs fixing — do not silently skip.
        let store = run_processor(&proc, &signed, "image/jpeg")
            .expect("EphemeralSigner-signed content must produce Ok output; fix test signer trust");

        // Output is the c2pa-rs Reader::json() JSON. Verify shape.
        assert!(store.is_object(), "output must be a JSON object");
        let obj = store.as_object().unwrap();
        assert!(
            obj.contains_key("active_manifest"),
            "store must contain active_manifest"
        );
        assert!(
            obj.contains_key("manifests"),
            "store must contain manifests map"
        );

        // The active manifest should mention our test claim_generator name —
        // walk the JSON tree rather than substring match (H4).
        let active_label = store["active_manifest"].as_str().unwrap();
        let manifest = &store["manifests"][active_label];
        let claim_gen = manifest["claim_generator"]
            .as_str()
            .or_else(|| manifest["claim_generator_info"][0]["name"].as_str())
            .expect("claim_generator or claim_generator_info[0].name must be present");
        assert!(
            claim_gen.contains("title-core-test"),
            "manifest should preserve the claim_generator name we signed with, got: {claim_gen}"
        );
    }

    #[test]
    fn process_unsigned_content_returns_error() {
        let unsigned = create_test_jpeg();
        let proc = C2paVerifyProcessor::new();
        let result = run_processor(&proc, &unsigned, "image/jpeg");
        assert!(result.is_err(), "unsigned content should return error");
        match result {
            Err(ProcessorError::C2paVerificationFailed(_)) => {} // expected
            other => panic!("Expected C2paVerificationFailed, got: {other:?}"),
        }
    }

    #[test]
    fn process_empty_content_returns_error() {
        let proc = C2paVerifyProcessor::new();
        let result = run_processor(&proc, &[], "image/jpeg");
        assert!(result.is_err(), "empty content should return error");
    }

    #[test]
    fn process_garbage_content_returns_error() {
        let proc = C2paVerifyProcessor::new();
        let result = run_processor(&proc, b"not a valid image", "image/jpeg");
        assert!(result.is_err(), "garbage content should return error");
    }

    // ── signature_hash tests ──

    #[test]
    fn signature_hash_deterministic() {
        let signed = create_signed_jpeg();

        let hash1 = compute_signature_hash_bytes(&signed, "image/jpeg")
            .expect("signature_hash computation");
        let hash2 = compute_signature_hash_bytes(&signed, "image/jpeg")
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

        let hash1 = compute_signature_hash_bytes(&signed1, "image/jpeg").unwrap();
        let hash2 = compute_signature_hash_bytes(&signed2, "image/jpeg").unwrap();

        // Different signing events → different COSE signatures → different hashes
        assert_ne!(
            hash1, hash2,
            "Different signing events should produce different signature_hashes"
        );
    }

    #[test]
    fn signature_hash_unsigned_content_error() {
        let unsigned = create_test_jpeg();
        let result = compute_signature_hash_bytes(&unsigned, "image/jpeg");
        assert!(
            result.is_err(),
            "unsigned content should not produce signature_hash"
        );
    }

    /// signature_hash 計算は seek が走っても結果が変わらないことを確認。
    /// Range Request reader は内部で seek するため、この性質が崩れると
    /// orchestrator の declared_signature_hash mismatch を誤検出する。
    #[test]
    fn signature_hash_idempotent_on_same_stream() {
        let signed = create_signed_jpeg();
        let source = InMemorySource::new(signed);
        let mut reader = source.open().unwrap();

        let h1 = compute_signature_hash(reader.as_mut(), "image/jpeg").unwrap();
        let h2 = compute_signature_hash(reader.as_mut(), "image/jpeg").unwrap();
        assert_eq!(h1, h2);
    }

    // ── manifest store shape (§3.2) ──

    #[test]
    fn output_is_manifest_store_with_status_unset() {
        let signed = create_signed_jpeg();
        let proc = C2paVerifyProcessor::new();
        let value = run_processor(&proc, &signed, "image/jpeg")
            .expect("EphemeralSigner-signed content must produce Ok output");

        // status は ProcessorOutput::ok() が後付けする層なので processor の生出力には含まれない
        assert!(value.get("status").is_none());
        // Top-level shape is the c2pa ManifestStore
        assert!(value.get("active_manifest").is_some());
        assert!(value.get("manifests").is_some());
    }

    /// 生 JSON 値はそのまま serde で round-trip 可能 (情報損失ゼロ)。
    #[test]
    fn output_roundtrip_lossless() {
        let signed = create_signed_jpeg();
        let proc = C2paVerifyProcessor::new();
        let value = run_processor(&proc, &signed, "image/jpeg")
            .expect("EphemeralSigner-signed content must produce Ok output");
        let re_serialized = serde_json::to_value(&value).unwrap();
        assert_eq!(value, re_serialized);
    }

    /// 複数 action を含む manifest を流すと、全 action が manifest_store 内に
    /// 完全保存される (元データから復元可能であることの担保)。
    /// H4: JSON ツリーを walk して action 名を厳密に検証する。
    #[test]
    fn manifest_store_preserves_all_actions() {
        let signed =
            create_signed_jpeg_with_actions(&["c2pa.created", "c2pa.edited", "c2pa.cropped"]);
        let proc = C2paVerifyProcessor::new();
        let value = run_processor(&proc, &signed, "image/jpeg")
            .expect("EphemeralSigner-signed content must produce Ok output");

        let active_label = value["active_manifest"].as_str().unwrap();
        let assertions = value["manifests"][active_label]["assertions"]
            .as_array()
            .expect("assertions must be an array");
        let actions_assertion = assertions
            .iter()
            .find(|a| {
                let label = a["label"].as_str().unwrap_or("");
                label == "c2pa.actions" || label == "c2pa.actions.v2"
            })
            .expect("actions assertion must be present");
        let actions = actions_assertion["data"]["actions"]
            .as_array()
            .expect("actions array must exist under data.actions");
        let names: Vec<&str> = actions
            .iter()
            .map(|a| a["action"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["c2pa.created", "c2pa.edited", "c2pa.cropped"]);
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

        let source = InMemorySource::new(signed);
        let results = registry.execute(&["c2pa-verify".into()], &source, "image/jpeg");

        assert_eq!(results.len(), 1);
        // EphemeralSigner content must validate to Ok under the pinned c2pa-rs.
        // If this ever fails, fix the test trust setup — do not weaken the gate.
        assert_eq!(
            results["c2pa-verify"].status,
            crate::response::ProcessorStatus::Ok,
            "EphemeralSigner-signed content must pass the strict gate"
        );
    }

    #[test]
    fn execute_via_registry_unsigned_content() {
        let unsigned = create_test_jpeg();
        let mut registry = crate::ProcessorRegistry::new();
        registry.register(Box::new(C2paVerifyProcessor::new()));

        let source = InMemorySource::new(unsigned);
        let results = registry.execute(&["c2pa-verify".into()], &source, "image/jpeg");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results["c2pa-verify"].status,
            crate::response::ProcessorStatus::Error,
            "unsigned content should produce error status"
        );
    }
}
