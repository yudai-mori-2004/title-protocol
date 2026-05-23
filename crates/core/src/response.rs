// SPDX-License-Identifier: Apache-2.0

//! # レスポンス型定義
//!
//! 仕様書 §2.3 — レスポンス形式
//!
//! TEEが返す属性抽出の処理結果。
//! `signature_hash` + `results` がAttestation Documentのuser_dataにバインドされる。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 処理結果のうち、Attestation Documentで封印される部分。
/// 仕様書 §2.3
///
/// JCS (JSON Canonicalization Scheme, RFC 8785) で正規化した上で
/// SHA-256ハッシュを計算し、Attestation Documentの `user_data` に埋め込む。
///
/// レスポンスの完全性検証は、この部分のハッシュとuser_dataの照合で行う。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableResponse {
    /// Active Manifestの署名のSHA-256ハッシュ。
    /// c2pa-verifyが算出するプロトコルレベルのコンテンツ識別子。
    /// 仕様書 §1.3
    pub signature_hash: String,

    /// 各processorの出力。キーはprocessor_id。
    /// 仕様書 §3.1 — あるprocessorがエラーでも他の結果は正常に返される。
    pub results: HashMap<String, ProcessorOutput>,
}

/// 属性抽出レスポンス（全体）。
/// 仕様書 §2.3
///
/// # JSON例
/// ```json
/// {
///   "signature_hash": "sha256:abcdef1234...",
///   "results": {
///     "c2pa-verify": { "status": "ok", "validation": "valid", ... },
///     "image-pdq": { "status": "ok", "pdqhash": "a95669d1..." }
///   },
///   "attestation": "(Base64エンコードされたAttestation Document)"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResponse {
    /// Attestation Documentで封印される検証可能な部分。
    #[serde(flatten)]
    pub verifiable: VerifiableResponse,

    /// Base64エンコードされたAttestation Document。
    /// 仕様書 §1.2 — user_dataに `verifiable` のハッシュが埋め込まれている。
    pub attestation: String,
}

/// 個別processorの出力。
/// 仕様書 §3.1
///
/// `status` フィールドで成否を示し、processor固有のデータは
/// `data` に格納されてトップレベルにフラット展開される。
///
/// # JSON例（成功時）
/// ```json
/// { "status": "ok", "validation": "valid", "signer": "Google" }
/// ```
///
/// # JSON例（エラー時）
/// ```json
/// { "status": "error", "error": "unsupported format" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorOutput {
    /// 処理の成否。
    pub status: ProcessorStatus,

    /// processor固有の出力データ。
    /// トップレベルにフラット展開される。
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Processorの処理状態。
/// 仕様書 §3.1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessorStatus {
    /// 処理成功。
    Ok,
    /// 処理失敗（他processorには影響しない）。
    Error,
}

impl ProcessorOutput {
    /// 成功時のProcessorOutputを構築する。
    /// 仕様書 §3.1
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            status: ProcessorStatus::Ok,
            data,
        }
    }

    /// エラー時のProcessorOutputを構築する。
    /// 仕様書 §3.1 — エラー情報を含むが、他processorの実行には影響しない。
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: ProcessorStatus::Error,
            data: serde_json::json!({ "error": message.into() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── §2.3 レスポンス形式 ──

    #[test]
    fn process_response_serialize_matches_spec() {
        let resp = ProcessResponse {
            verifiable: VerifiableResponse {
                signature_hash: "sha256:abcdef1234".into(),
                results: {
                    let mut m = HashMap::new();
                    m.insert(
                        "c2pa-verify".into(),
                        ProcessorOutput::ok(serde_json::json!({
                            "validation": "valid",
                            "signer": "Google",
                            "timestamp": "2026-01-15T10:30:00Z"
                        })),
                    );
                    m.insert(
                        "image-pdq".into(),
                        ProcessorOutput::ok(serde_json::json!({
                            "pdqhash": "a95669d1"
                        })),
                    );
                    m
                },
            },
            attestation: "base64attestation".into(),
        };

        let json = serde_json::to_value(&resp).unwrap();

        // signature_hash はトップレベルに展開（flatten）
        assert_eq!(json["signature_hash"], "sha256:abcdef1234");
        assert_eq!(json["attestation"], "base64attestation");

        // results の中身
        let c2pa = &json["results"]["c2pa-verify"];
        assert_eq!(c2pa["status"], "ok");
        assert_eq!(c2pa["validation"], "valid");

        let pdq = &json["results"]["image-pdq"];
        assert_eq!(pdq["status"], "ok");
        assert_eq!(pdq["pdqhash"], "a95669d1");
    }

    #[test]
    fn process_response_deserialize_from_spec() {
        let json = r#"{
            "signature_hash": "sha256:abcdef1234",
            "results": {
                "c2pa-verify": {
                    "status": "ok",
                    "validation": "valid",
                    "signer": "Google",
                    "timestamp": "2026-01-15T10:30:00Z"
                },
                "image-pdq": {
                    "status": "ok",
                    "pdqhash": "a95669d1"
                }
            },
            "attestation": "base64data"
        }"#;

        let resp: ProcessResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.verifiable.signature_hash, "sha256:abcdef1234");
        assert_eq!(resp.attestation, "base64data");
        assert_eq!(resp.verifiable.results.len(), 2);

        let c2pa = &resp.verifiable.results["c2pa-verify"];
        assert_eq!(c2pa.status, ProcessorStatus::Ok);
    }

    #[test]
    fn process_response_roundtrip() {
        let resp = ProcessResponse {
            verifiable: VerifiableResponse {
                signature_hash: "sha256:test".into(),
                results: {
                    let mut m = HashMap::new();
                    m.insert("c2pa-verify".into(), ProcessorOutput::ok(serde_json::json!({"validation": "valid"})));
                    m
                },
            },
            attestation: "att".into(),
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let restored: ProcessResponse = serde_json::from_str(&json_str).unwrap();

        assert_eq!(resp.verifiable.signature_hash, restored.verifiable.signature_hash);
        assert_eq!(resp.attestation, restored.attestation);
        assert_eq!(resp.verifiable.results.len(), restored.verifiable.results.len());
    }

    // ── ProcessorOutput ──

    #[test]
    fn processor_output_ok_flatten() {
        let output = ProcessorOutput::ok(serde_json::json!({
            "validation": "valid",
            "signer": "Google"
        }));
        let json = serde_json::to_value(&output).unwrap();

        assert_eq!(json["status"], "ok");
        assert_eq!(json["validation"], "valid");
        assert_eq!(json["signer"], "Google");
        // "data" キーは存在しない（flatten）
        assert!(json.get("data").is_none());
    }

    #[test]
    fn processor_output_error_flatten() {
        let output = ProcessorOutput::error("unsupported format");
        let json = serde_json::to_value(&output).unwrap();

        assert_eq!(json["status"], "error");
        assert_eq!(json["error"], "unsupported format");
        assert!(json.get("data").is_none());
    }

    #[test]
    fn processor_output_error_roundtrip() {
        let output = ProcessorOutput::error("test error");
        let json_str = serde_json::to_string(&output).unwrap();
        let restored: ProcessorOutput = serde_json::from_str(&json_str).unwrap();
        assert_eq!(restored.status, ProcessorStatus::Error);
    }

    // ── §3.1 エラー時も他processorに影響しない ──

    #[test]
    fn mixed_ok_and_error_results() {
        let json = r#"{
            "signature_hash": "sha256:test",
            "results": {
                "c2pa-verify": { "status": "ok", "validation": "valid" },
                "image-pdq": { "status": "ok", "pdqhash": "abc" },
                "some-proc": { "status": "error", "error": "unsupported format" }
            },
            "attestation": "att"
        }"#;

        let resp: ProcessResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.verifiable.results.len(), 3);
        assert_eq!(
            resp.verifiable.results["c2pa-verify"].status,
            ProcessorStatus::Ok
        );
        assert_eq!(
            resp.verifiable.results["some-proc"].status,
            ProcessorStatus::Error
        );
    }
}
