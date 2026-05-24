// SPDX-License-Identifier: Apache-2.0

//! # レスポンス型定義
//!
//! 仕様書 §2.3 — レスポンス形式
//!
//! TEEが返す属性抽出の処理結果。
//! `signature_hash` + `results` がAttestation Documentのuser_dataにバインドされる。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

    /// processor固有の出力データ。トップレベルにフラット展開される。
    ///
    /// 型を `Map<String, Value>` に固定することで、`#[serde(flatten)]` が
    /// 要求する「object 形」を型レベルで強制する。`Value` (配列・スカラー)
    /// を直接代入して serialize 時に panic する経路を物理的に塞ぐ。
    #[serde(flatten)]
    pub data: Map<String, Value>,
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
    /// Build a successful `ProcessorOutput`. Spec §3.1.
    ///
    /// `data` は JSON object であることを型 (`Map<String, Value>`) で要求する。
    /// 旧 API では `Value` を受けて `is_object()` ガードで非 object を弾いていた
    /// が、`pub data: Value` への直接代入経路が型から塞がれず、配列やスカラーを
    /// 直接代入すると serialize 時に panic していた。`Map` 化でこの経路は不可。
    ///
    /// 既存呼び出し側との互換性のため、`serde_json::json!({ ... })` のような
    /// オブジェクトリテラル渡しには `from_value_object` を併用する。
    pub fn ok(data: Map<String, Value>) -> Self {
        Self {
            status: ProcessorStatus::Ok,
            data,
        }
    }

    /// `serde_json::json!({ ... })` リテラルから `ProcessorOutput::ok` を作る
    /// 便利ヘルパ。非 object を渡した場合は `error` variant に落とす。
    pub fn from_value_object(value: Value) -> Self {
        match value {
            Value::Object(map) => Self::ok(map),
            other => Self::error(format!("processor returned non-object data: {other}")),
        }
    }

    /// Error variant — recorded inline so the rest of the response stays
    /// well-formed. Spec §3.1 (one processor failing doesn't affect others).
    pub fn error(message: impl Into<String>) -> Self {
        let mut data = Map::new();
        data.insert("error".to_string(), Value::String(message.into()));
        Self {
            status: ProcessorStatus::Error,
            data,
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
                        ProcessorOutput::from_value_object(serde_json::json!({
                            "validation": "valid",
                            "signer": "Google",
                            "timestamp": "2026-01-15T10:30:00Z"
                        })),
                    );
                    m.insert(
                        "image-pdq".into(),
                        ProcessorOutput::from_value_object(serde_json::json!({
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
                    m.insert(
                        "c2pa-verify".into(),
                        ProcessorOutput::from_value_object(serde_json::json!({"validation": "valid"})),
                    );
                    m
                },
            },
            attestation: "att".into(),
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let restored: ProcessResponse = serde_json::from_str(&json_str).unwrap();

        assert_eq!(
            resp.verifiable.signature_hash,
            restored.verifiable.signature_hash
        );
        assert_eq!(resp.attestation, restored.attestation);
        assert_eq!(
            resp.verifiable.results.len(),
            restored.verifiable.results.len()
        );
    }

    // ── ProcessorOutput ──

    #[test]
    fn processor_output_ok_flatten() {
        let output = ProcessorOutput::from_value_object(serde_json::json!({
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
