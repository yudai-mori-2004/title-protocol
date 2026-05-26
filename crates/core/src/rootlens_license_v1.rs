// SPDX-License-Identifier: Apache-2.0

//! # rootlens-license-v1 Processor
//!
//! Task 18 — RootLens Root NFT 発行パイプライン用 processor。
//!
//! C2PA 署名検証 (c2pa-verify と同等) + CAWG training-mining assertion ゲート
//! + RootLens サブライセンスバインディングを単一 processor で提供する。
//! RootLens はこの processor だけ指定すれば Root NFT 発行に必要な全属性が得られる。

use crate::content_stream::ContentStream;
use crate::processor::{Processor, ProcessorError};
use serde::{Deserialize, Serialize};
use std::io::SeekFrom;

/// Minimal shape of a CAWG training-mining assertion (v1) needed to gate the
/// processor. We do not interpret the policy beyond requiring the structural
/// invariants the spec mandates; downstream consumers do policy interpretation
/// from the full assertion in `manifest_store`. The point of validating
/// structure here is to refuse a placeholder assertion (e.g. `data: null` or
/// `entries: []`) that would otherwise pass a label-only presence check.
#[derive(Deserialize)]
struct CawgTrainingMiningAssertion {
    entries: Vec<CawgTrainingMiningEntry>,
}

#[derive(Deserialize)]
struct CawgTrainingMiningEntry {
    /// CAWG v1 mandates `use` ∈ {"allowed", "notAllowed", "constrained"}. We
    /// only require the field's presence and non-emptiness so that future spec
    /// additions don't force a TP rebuild.
    #[serde(rename = "use")]
    #[allow(dead_code)] // presence-only check, value retained for clarity
    use_: String,
}

pub const ROOTLENS_LICENSE_V1_PROCESSOR_ID: &str = "rootlens-license-v1";

const BINDING_PROTOCOL_VERSION: &str = "rootlens-license-v1";
const PURPOSE: &str = "sublicense-grant-eligibility";
const LICENSE_PROGRAM_ID: &str = "G1PWd1nMe63isDaYT3iijcyWac9d4RE1CBrvaKZFjpV8";
const LICENSE_COLLECTION_MINT: &str = "BvhuJiTWDW6n5cSzE4XmzYcwLry7vcstS1U7fD7n9N1b";
const LICENSE_NFT_TERMS_URL_TEMPLATE: &str =
    "https://rootlens.io/licenses/{type}/{terms_hash}.json";
const TOS_VERSION: &str = "v1.0.0";
const TOS_CONSENT_LOG_ENDPOINT: &str = "https://www.rootlens.io/api/v1/tos/consent";

const CAWG_TRAINING_MINING_LABEL: &str = "cawg.training-mining";

/// rootlens-license-v1 processor 出力。
///
/// `c2pa` フィールドは、検証済の C2PA マニフェストストア本体 (c2pa-rs の
/// `Reader::json()` がそのまま返す C2PA 標準レイアウト)。validity の signal は
/// 「フィールドの存在そのもの」— processor が Ok を返した時点で、署名検証は
/// `Valid` または `Trusted` を通過している。読む側は C2PA パーサで内部の
/// `active_manifest` / `manifests` / `validation_results` 等を辿る。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootLensLicenseV1Output {
    /// 完全な C2PA マニフェストストア。c2pa-rs `Reader::json()` 出力そのもの。
    pub c2pa: serde_json::Value,
    /// RootLens サブライセンス枠組みメタデータ。
    pub rootlens_binding: RootLensBinding,
}

/// RootLens サブライセンス枠組みメタデータ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootLensBinding {
    pub binding_protocol_version: String,
    pub purpose: String,
    pub license_program_id: String,
    pub license_collection_mint: String,
    pub license_nft_terms_url_template: String,
    pub tos_version: String,
    pub tos_consent_log_endpoint: String,
}

/// rootlens-license-v1 processor。
/// Task 18
pub struct RootLensLicenseV1Processor;

impl RootLensLicenseV1Processor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RootLensLicenseV1Processor {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for RootLensLicenseV1Processor {
    fn id(&self) -> &str {
        ROOTLENS_LICENSE_V1_PROCESSOR_ID
    }

    /// C2PA 検証 (Valid/Trusted のみ通過) → CAWG TDM ゲート → manifest_store
    /// と rootlens_binding を一体で出力。
    ///
    /// Invalid 署名、Active Manifest 不在、`cawg.training-mining` assertion 不在
    /// のいずれも `ProcessorError` で reject する。reject 時は manifest を一切
    /// 出力に含めない。
    fn process(
        &self,
        content: &mut dyn ContentStream,
        content_type: &str,
    ) -> Result<serde_json::Value, ProcessorError> {
        // Range Request reader 等の場合に備えて先頭へ巻き戻す。
        content.seek(SeekFrom::Start(0)).map_err(|e| {
            ProcessorError::ReadFailed(format!("Failed to rewind content stream: {e}"))
        })?;
        let reader = c2pa::Reader::from_context(crate::c2pa_verify::c2pa_context()?)
            .with_stream(content_type, &mut *content)
            .map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!(
                    "C2PA Reader construction failed: {e}"
                ))
            })?;

        // ── C2PA 検証ゲート: Active Manifest 不在 / Invalid 署名は reject ──

        let manifest = reader.active_manifest().ok_or_else(|| {
            ProcessorError::C2paVerificationFailed("Active Manifest not found".to_string())
        })?;

        match reader.validation_state() {
            c2pa::validation_results::ValidationState::Valid
            | c2pa::validation_results::ValidationState::Trusted => {}
            c2pa::validation_results::ValidationState::Invalid => {
                return Err(ProcessorError::C2paVerificationFailed(
                    "C2PA validation failed (validation_state = Invalid)".to_string(),
                ));
            }
        }

        // ── CAWG TDM ゲート: assertion 不在 OR 構造不備は reject ──
        //
        // ゲート順序: Active Manifest 存在 → 署名 valid → assertion 構造検証。
        // 偽造された assertion 内容を信頼しないため、Invalid 通過後にしか
        // assertion を見ない。ラベルだけの存在チェックでは
        // `{"label": "cawg.training-mining", "data": null}` が通過してしまうので、
        // 最小構造 (entries が非空・各 entry に use フィールド) を要求する。
        let tdm: CawgTrainingMiningAssertion = manifest
            .find_assertion(CAWG_TRAINING_MINING_LABEL)
            .map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!(
                    "CAWG training-mining assertion ({CAWG_TRAINING_MINING_LABEL}) not found or malformed in C2PA manifest: {e}"
                ))
            })?;
        if tdm.entries.is_empty() {
            return Err(ProcessorError::C2paVerificationFailed(format!(
                "CAWG training-mining assertion ({CAWG_TRAINING_MINING_LABEL}) has empty `entries`; gate requires at least one entry"
            )));
        }

        // ── ここまで通過したので manifest store を完全保存 + rootlens_binding 出力 ──

        let c2pa: serde_json::Value = serde_json::from_str(&reader.json()).map_err(|e| {
            ProcessorError::Internal(format!("Failed to parse c2pa reader JSON: {e}"))
        })?;

        let output = RootLensLicenseV1Output {
            c2pa,
            rootlens_binding: RootLensBinding {
                binding_protocol_version: BINDING_PROTOCOL_VERSION.to_string(),
                purpose: PURPOSE.to_string(),
                license_program_id: LICENSE_PROGRAM_ID.to_string(),
                license_collection_mint: LICENSE_COLLECTION_MINT.to_string(),
                license_nft_terms_url_template: LICENSE_NFT_TERMS_URL_TEMPLATE.to_string(),
                tos_version: TOS_VERSION.to_string(),
                tos_consent_log_endpoint: TOS_CONSENT_LOG_ENDPOINT.to_string(),
            },
        };

        serde_json::to_value(&output).map_err(|e| {
            ProcessorError::Internal(format!(
                "Failed to serialize RootLensLicenseV1Output: {e}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_stream::{ContentSource, InMemorySource};
    use std::io::Cursor;

    /// テスト用に `Vec<u8>` を InMemorySource にしてから processor に渡すヘルパ。
    /// `processor.process(&mut reader, content_type)` を呼ぶ。
    fn run_processor(
        processor: &RootLensLicenseV1Processor,
        bytes: &[u8],
        content_type: &str,
    ) -> Result<serde_json::Value, ProcessorError> {
        let source = InMemorySource::new(bytes.to_vec());
        let mut reader = source.open().expect("InMemorySource::open never fails");
        processor.process(reader.as_mut(), content_type)
    }

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

    fn create_signed_jpeg_with_tdm() -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer =
            c2pa::EphemeralSigner::new("title-core-test").expect("EphemeralSigner creation");

        let definition = serde_json::json!({
            "claim_generator_info": [{
                "name": "title-core-test",
                "version": "0.1.2"
            }],
            "assertions": [
                {
                    "label": "c2pa.actions.v2",
                    "data": {
                        "actions": [{ "action": "c2pa.created" }]
                    }
                },
                {
                    "label": "cawg.training-mining",
                    "data": {
                        "metadata": {
                            "dateTime": "2026-05-24T00:00:00Z"
                        },
                        "entries": [{
                            "use": "notAllowed",
                            "constraint_info": "All rights reserved. No TDM permitted."
                        }]
                    }
                }
            ]
        });

        let mut builder = c2pa::Builder::from_context(c2pa::Context::default())
            .with_definition(definition.to_string())
            .expect("Builder creation");

        let mut source = Cursor::new(&test_jpeg);
        let mut dest = Cursor::new(Vec::new());
        builder
            .sign(&signer, "image/jpeg", &mut source, &mut dest)
            .expect("C2PA signing with TDM");
        dest.into_inner()
    }

    /// 共通: 与えられた TDM data ペイロードで署名済 JPEG を作るヘルパ。
    /// H2 回帰テスト用に、`entries: []` や `null` 等の不正ペイロードを差し込めるようにする。
    fn create_signed_jpeg_with_tdm_data(tdm_data: serde_json::Value) -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer =
            c2pa::EphemeralSigner::new("title-core-test").expect("EphemeralSigner creation");

        let definition = serde_json::json!({
            "claim_generator_info": [{"name": "title-core-test", "version": "0.1.2"}],
            "assertions": [
                { "label": "c2pa.actions.v2", "data": {"actions": [{"action": "c2pa.created"}]} },
                { "label": "cawg.training-mining", "data": tdm_data }
            ]
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

    fn create_signed_jpeg_with_empty_tdm() -> Vec<u8> {
        create_signed_jpeg_with_tdm_data(serde_json::json!({"entries": []}))
    }

    fn create_signed_jpeg_with_null_tdm() -> Vec<u8> {
        create_signed_jpeg_with_tdm_data(serde_json::json!(null))
    }

    fn create_signed_jpeg_without_tdm() -> Vec<u8> {
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
                    "actions": [{ "action": "c2pa.created" }]
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
            .expect("C2PA signing without TDM");
        dest.into_inner()
    }

    #[test]
    fn processor_id() {
        let proc = RootLensLicenseV1Processor::new();
        assert_eq!(proc.id(), "rootlens-license-v1");
    }

    #[test]
    fn process_with_tdm_produces_c2pa_and_binding() {
        let signed = create_signed_jpeg_with_tdm();
        let proc = RootLensLicenseV1Processor::new();
        // EphemeralSigner is expected to validate as Valid/Trusted under the
        // pinned c2pa-rs version. If this unwrap ever panics, the test trust
        // setup needs fixing (do not silently skip — that loses coverage of
        // every success-path assertion that follows).
        let value = run_processor(&proc, &signed, "image/jpeg")
            .expect("EphemeralSigner-signed content must produce Ok output; fix test signer trust");
        let output: RootLensLicenseV1Output =
            serde_json::from_value(value).expect("deserialize output");

        // c2pa field is the manifest store directly (c2pa-rs Reader::json shape).
        assert!(output.c2pa.is_object(), "c2pa must be a JSON object");
        let store = output.c2pa.as_object().unwrap();
        assert!(
            store.contains_key("active_manifest"),
            "manifest store should expose active_manifest pointer"
        );
        assert!(
            store.contains_key("manifests"),
            "manifest store should expose manifests map"
        );

        // RootLens binding
        assert_eq!(
            output.rootlens_binding.binding_protocol_version,
            "rootlens-license-v1"
        );
        assert_eq!(
            output.rootlens_binding.purpose,
            "sublicense-grant-eligibility"
        );
        assert_eq!(
            output.rootlens_binding.license_program_id,
            "G1PWd1nMe63isDaYT3iijcyWac9d4RE1CBrvaKZFjpV8"
        );
        assert_eq!(
            output.rootlens_binding.license_collection_mint,
            "BvhuJiTWDW6n5cSzE4XmzYcwLry7vcstS1U7fD7n9N1b"
        );
        assert_eq!(output.rootlens_binding.tos_version, "v1.0.0");
    }

    #[test]
    fn process_without_tdm_assertion_fails() {
        let signed = create_signed_jpeg_without_tdm();
        let proc = RootLensLicenseV1Processor::new();
        let result = run_processor(&proc, &signed, "image/jpeg");
        assert!(result.is_err(), "should fail without TDM assertion");
        match result {
            Err(ProcessorError::C2paVerificationFailed(msg)) => {
                assert!(
                    msg.contains("cawg.training-mining"),
                    "error should mention cawg.training-mining: {msg}"
                );
            }
            other => panic!("expected C2paVerificationFailed, got: {other:?}"),
        }
    }

    #[test]
    fn process_no_c2pa_manifest_fails() {
        let unsigned = create_test_jpeg();
        let proc = RootLensLicenseV1Processor::new();
        let result = run_processor(&proc, &unsigned, "image/jpeg");
        assert!(result.is_err(), "should fail without C2PA manifest");
    }

    /// 同じ reader を 2 回 process しても結果が変わらないことを確認。
    /// processor 内部で stream を先頭まで巻き戻すので、Range Request reader でも
    /// 安全に再利用できる契約。
    #[test]
    fn process_rewinds_stream_each_call() {
        let signed = create_signed_jpeg_with_tdm();
        let proc = RootLensLicenseV1Processor::new();
        let source = InMemorySource::new(signed.clone());
        let mut reader = source.open().unwrap();

        let v1 = proc.process(reader.as_mut(), "image/jpeg").unwrap();
        let v2 = proc.process(reader.as_mut(), "image/jpeg").unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn output_deterministic() {
        let signed = create_signed_jpeg_with_tdm();
        let proc = RootLensLicenseV1Processor::new();
        let v1 = run_processor(&proc, &signed, "image/jpeg").unwrap();
        let v2 = run_processor(&proc, &signed, "image/jpeg").unwrap();
        assert_eq!(v1, v2);
    }

    /// `c2pa` フィールド (= manifest store 本体) に CAWG TDM assertion が
    /// そのまま保存されていることを確認。コンテンツデータ無しでも、TEE 出力から
    /// `cawg.training-mining` の存在と内容を再検証できる。
    #[test]
    fn manifest_store_preserves_cawg_tdm_assertion() {
        let signed = create_signed_jpeg_with_tdm();
        let proc = RootLensLicenseV1Processor::new();
        let value = run_processor(&proc, &signed, "image/jpeg")
            .expect("EphemeralSigner-signed content must produce Ok output");
        let output: RootLensLicenseV1Output = serde_json::from_value(value).unwrap();

        // H4: walk the manifest store JSON tree instead of substring matching.
        // assertions live under `manifests[<label>].assertions[]`.
        let manifests = output.c2pa["manifests"]
            .as_object()
            .expect("manifests must be an object");
        let active_label = output.c2pa["active_manifest"]
            .as_str()
            .expect("active_manifest must be a string");
        let manifest = manifests
            .get(active_label)
            .expect("active manifest must exist in manifests map");
        let assertions = manifest["assertions"]
            .as_array()
            .expect("assertions must be an array");

        let tdm_assertion = assertions
            .iter()
            .find(|a| a["label"].as_str() == Some("cawg.training-mining"))
            .expect("cawg.training-mining assertion must be present in active manifest");

        let entries = tdm_assertion["data"]["entries"]
            .as_array()
            .expect("TDM data.entries must be a non-empty array");
        assert!(!entries.is_empty(), "TDM entries must not be empty");

        let first_use = entries[0]["use"]
            .as_str()
            .expect("TDM entries[0].use must be a string");
        assert_eq!(
            first_use, "notAllowed",
            "TDM use value must survive into manifest store verbatim"
        );
    }

    /// H2 回帰テスト: ラベル付きでも entries が空の TDM assertion は reject される。
    #[test]
    fn empty_tdm_entries_rejected() {
        let signed = create_signed_jpeg_with_empty_tdm();
        let proc = RootLensLicenseV1Processor::new();
        match run_processor(&proc, &signed, "image/jpeg") {
            Err(ProcessorError::C2paVerificationFailed(msg)) => {
                assert!(
                    msg.contains("entries") || msg.contains("malformed"),
                    "error should mention empty entries / malformed structure: {msg}"
                );
            }
            other => panic!("expected rejection of empty TDM entries, got: {other:?}"),
        }
    }

    /// H2 回帰テスト: `data: null` のような構造的に欠陥のある TDM assertion も
    /// reject される。
    #[test]
    fn null_tdm_data_rejected() {
        let signed = create_signed_jpeg_with_null_tdm();
        let proc = RootLensLicenseV1Processor::new();
        match run_processor(&proc, &signed, "image/jpeg") {
            Err(ProcessorError::C2paVerificationFailed(_)) => {} // expected
            other => panic!("expected rejection of null TDM data, got: {other:?}"),
        }
    }

    #[test]
    fn register_in_processor_registry() {
        let mut registry = crate::ProcessorRegistry::new();
        registry.register(Box::new(RootLensLicenseV1Processor::new()));
        assert!(registry.processor_ids().contains(&"rootlens-license-v1"));
        assert!(registry.get("rootlens-license-v1").is_some());
    }

    #[test]
    fn output_json_schema() {
        let signed = create_signed_jpeg_with_tdm();
        let proc = RootLensLicenseV1Processor::new();
        let value = run_processor(&proc, &signed, "image/jpeg")
            .expect("rootlens-license-v1 must accept signed+TDM content");

        // Top level: { c2pa: <manifest store>, rootlens_binding: {...} }
        assert!(value.get("c2pa").is_some(), "c2pa field must be present");
        let c2pa = value.get("c2pa").unwrap();
        assert!(c2pa.get("active_manifest").is_some());
        assert!(c2pa.get("manifests").is_some());
        // No vestigial convenience fields at the c2pa level
        assert!(c2pa.get("validation").is_none());
        assert!(c2pa.get("signer").is_none());
        assert!(c2pa.get("actions").is_none());

        assert!(value.get("rootlens_binding").is_some());
        let binding = value.get("rootlens_binding").unwrap();
        assert!(binding.get("binding_protocol_version").is_some());
        assert!(binding.get("purpose").is_some());
        assert!(binding.get("license_program_id").is_some());
        assert!(binding.get("license_collection_mint").is_some());
        assert!(binding.get("license_nft_terms_url_template").is_some());
        assert!(binding.get("tos_version").is_some());
        assert!(binding.get("tos_consent_log_endpoint").is_some());

        assert!(value.get("status").is_none());
        assert!(value.get("content_hash").is_none());
    }

    /// `rootlens-license-v1` の `c2pa` フィールドは、standalone の
    /// `c2pa-verify` processor 出力 (= manifest store そのもの) と完全一致する。
    /// 抽出ロジックの重複を持たない設計の担保。
    #[test]
    fn c2pa_output_matches_standalone_c2pa_verify() {
        let signed = create_signed_jpeg_with_tdm();

        let rootlens = RootLensLicenseV1Processor::new();
        let rootlens_val = run_processor(&rootlens, &signed, "image/jpeg")
            .expect("rootlens-license-v1 must accept signed+TDM content");
        let rootlens_c2pa = rootlens_val.get("c2pa").unwrap();

        let c2pa_source = InMemorySource::new(signed.clone());
        let mut c2pa_reader = c2pa_source.open().unwrap();
        let c2pa_proc = crate::C2paVerifyProcessor::new();
        let c2pa_val = c2pa_proc
            .process(c2pa_reader.as_mut(), "image/jpeg")
            .expect("c2pa-verify must accept signed content");

        assert_eq!(
            rootlens_c2pa, &c2pa_val,
            "c2pa field must equal standalone c2pa-verify output (both are the manifest store)"
        );
    }
}
