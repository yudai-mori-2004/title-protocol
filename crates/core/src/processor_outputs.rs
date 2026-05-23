// SPDX-License-Identifier: Apache-2.0

//! # Processor出力型定義
//!
//! 仕様書 §3.2 — 各processorの出力JSON構造に対応する型。
//!
//! 各processorは `serde_json::Value` を返すが、ここで定義する型を使うことで
//! 型安全にprocessor固有の出力を構築・パースできる。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// c2pa-verify (§3.2)
// ---------------------------------------------------------------------------

/// c2pa-verify processorの出力。
/// 仕様書 §3.2 c2pa-verify
///
/// C2PA署名チェーンの検証結果とマニフェスト情報。
///
/// # JSON例
/// ```json
/// {
///   "validation": "valid",
///   "signer": { "issuer": "Google LLC", "cert_serial": "..." },
///   "timestamp": "2026-01-15T10:30:00Z",
///   "claim_generator": "Google Pixel 10 Camera",
///   "actions": [{ "action": "c2pa.created" }]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct C2paVerifyOutput {
    /// 検証結果。`"valid"` または `"invalid"`。
    /// 署名が無効でもprocessor自体はエラーにならない。
    pub validation: String,

    /// 署名者情報。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerInfo>,

    /// C2PAタイムスタンプ（ISO 8601形式）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    /// Claim Generator（署名ツール/デバイス名）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_generator: Option<String>,

    /// アクション履歴。
    #[serde(default)]
    pub actions: Vec<C2paAction>,
}

/// C2PA署名者情報。
/// 仕様書 §3.2 c2pa-verify
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerInfo {
    /// 証明書の発行者名。
    pub issuer: String,

    /// 証明書のシリアル番号。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_serial: Option<String>,
}

/// C2PAアクション情報。
/// 仕様書 §3.2 c2pa-verify
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct C2paAction {
    /// アクション種別（例: `"c2pa.created"`, `"c2pa.edited"`）。
    pub action: String,
}

// ---------------------------------------------------------------------------
// provenance-graph (§3.2)
// ---------------------------------------------------------------------------

/// provenance-graph processorの出力。
/// 仕様書 §3.2 provenance-graph
///
/// C2PAマニフェストから素材情報を再帰的に抽出した有向非巡回グラフ（DAG）。
///
/// # JSON例
/// ```json
/// {
///   "nodes": [
///     { "id": "sha256:abcd1234...", "title": "final_video.mp4" }
///   ],
///   "edges": [
///     { "source": "sha256:ef567890...", "target": "sha256:abcd1234...", "role": "audio" }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceGraphOutput {
    /// グラフのノード一覧。
    pub nodes: Vec<GraphNode>,

    /// グラフのエッジ一覧（素材→派生の関係）。
    pub edges: Vec<GraphEdge>,
}

/// 来歴グラフのノード。
/// 仕様書 §3.2 provenance-graph
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// ノードID（signature_hashまたはcontent_hash）。
    pub id: String,

    /// コンテンツのタイトル。
    pub title: String,
}

/// 来歴グラフのエッジ。
/// 仕様書 §3.2 provenance-graph
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    /// 素材のID。
    pub source: String,

    /// 派生物のID。
    pub target: String,

    /// 関係の種類（例: `"audio"`, `"image"`）。
    pub role: String,
}

// ---------------------------------------------------------------------------
// image-pdq (§3.2)
// ---------------------------------------------------------------------------

/// image-pdq processorの出力。
/// 仕様書 §3.2 image-pdq
///
/// 画像のPDQ知覚ハッシュ（256ビット）と品質スコア。
///
/// # JSON例
/// ```json
/// {
///   "pdqhash": "a95669d1...",
///   "quality": 85
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePdqOutput {
    /// PDQハッシュ値（256ビット、hex文字列）。
    pub pdqhash: String,

    /// 品質スコア（0-100）。
    pub quality: u32,
}

// ---------------------------------------------------------------------------
// video-vpdq (§3.2)
// ---------------------------------------------------------------------------

/// video-vpdq processorの出力。
/// 仕様書 §3.2 video-vpdq
///
/// 動画の各フレームに対するPDQハッシュ列。
///
/// # JSON例
/// ```json
/// {
///   "frame_hashes": [
///     { "frame": 0, "timestamp_ms": 0, "pdqhash": "...", "quality": 90 }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoVpdqOutput {
    /// フレームごとのPDQハッシュ一覧。
    pub frame_hashes: Vec<FrameHash>,
}

/// フレーム単位のPDQハッシュ情報。
/// 仕様書 §3.2 video-vpdq
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHash {
    /// フレーム番号（0始まり）。
    pub frame: u64,

    /// フレームのタイムスタンプ（ミリ秒）。
    pub timestamp_ms: u64,

    /// PDQハッシュ値（256ビット、hex文字列）。
    pub pdqhash: String,

    /// 品質スコア（0-100）。
    pub quality: u32,
}

// ---------------------------------------------------------------------------
// cert-* (§3.2)
// ---------------------------------------------------------------------------

/// cert-* processorの出力。
/// 仕様書 §3.2 cert-google, cert-sony, cert-leica
///
/// C2PA署名の証明書チェーンが特定のルート証明書に連鎖するかの検証結果。
///
/// # JSON例
/// ```json
/// {
///   "verified": true,
///   "chain": [
///     { "subject": "Google Pixel 10", "issuer": "Google C2PA ICA G3" },
///     { "subject": "Google C2PA ICA G3", "issuer": "Google C2PA Root CA G3" }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertVerifyOutput {
    /// ルート証明書への連鎖が確認できたか。
    pub verified: bool,

    /// 証明書チェーンの詳細。
    #[serde(default)]
    pub chain: Vec<CertChainEntry>,
}

/// 証明書チェーンの1エントリ。
/// 仕様書 §3.2 cert-*
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertChainEntry {
    /// 証明書のサブジェクト名。
    pub subject: String,

    /// 証明書の発行者名。
    pub issuer: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── §3.2 c2pa-verify 出力 ──

    #[test]
    fn c2pa_verify_output_from_spec() {
        let json = r#"{
            "validation": "valid",
            "signer": {
                "issuer": "Google LLC",
                "cert_serial": "abc123"
            },
            "timestamp": "2026-01-15T10:30:00Z",
            "claim_generator": "Google Pixel 10 Camera",
            "actions": [
                { "action": "c2pa.created" }
            ]
        }"#;
        let output: C2paVerifyOutput = serde_json::from_str(json).unwrap();

        assert_eq!(output.validation, "valid");
        assert_eq!(output.signer.as_ref().unwrap().issuer, "Google LLC");
        assert_eq!(output.timestamp.as_deref(), Some("2026-01-15T10:30:00Z"));
        assert_eq!(
            output.claim_generator.as_deref(),
            Some("Google Pixel 10 Camera")
        );
        assert_eq!(output.actions.len(), 1);
        assert_eq!(output.actions[0].action, "c2pa.created");
    }

    #[test]
    fn c2pa_verify_output_roundtrip() {
        let output = C2paVerifyOutput {
            validation: "valid".into(),
            signer: Some(SignerInfo {
                issuer: "Test".into(),
                cert_serial: Some("serial".into()),
            }),
            timestamp: Some("2026-01-15T10:30:00Z".into()),
            claim_generator: Some("Test Camera".into()),
            actions: vec![C2paAction {
                action: "c2pa.created".into(),
            }],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        let restored: C2paVerifyOutput = serde_json::from_str(&json_str).unwrap();
        assert_eq!(output, restored);
    }

    #[test]
    fn c2pa_verify_output_optional_fields_omitted() {
        let output = C2paVerifyOutput {
            validation: "invalid".into(),
            signer: None,
            timestamp: None,
            claim_generator: None,
            actions: vec![],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(!json_str.contains("signer"));
        assert!(!json_str.contains("timestamp"));
        assert!(!json_str.contains("claim_generator"));
    }

    // ── §3.2 provenance-graph 出力 ──

    #[test]
    fn provenance_graph_output_from_spec() {
        let json = r#"{
            "nodes": [
                { "id": "sha256:abcd1234", "title": "final_video.mp4" },
                { "id": "sha256:ef567890", "title": "background_music.mp3" },
                { "id": "sha256:1234abcd", "title": "photo.jpg" }
            ],
            "edges": [
                { "source": "sha256:ef567890", "target": "sha256:abcd1234", "role": "audio" },
                { "source": "sha256:1234abcd", "target": "sha256:abcd1234", "role": "image" }
            ]
        }"#;
        let output: ProvenanceGraphOutput = serde_json::from_str(json).unwrap();

        assert_eq!(output.nodes.len(), 3);
        assert_eq!(output.edges.len(), 2);
        assert_eq!(output.edges[0].role, "audio");
    }

    // ── §3.2 image-pdq 出力 ──

    #[test]
    fn image_pdq_output_from_spec() {
        let json = r#"{
            "pdqhash": "a95669d1",
            "quality": 85
        }"#;
        let output: ImagePdqOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.pdqhash, "a95669d1");
        assert_eq!(output.quality, 85);
    }

    #[test]
    fn image_pdq_output_roundtrip() {
        let output = ImagePdqOutput {
            pdqhash: "test_hash".into(),
            quality: 90,
        };
        let json_str = serde_json::to_string(&output).unwrap();
        let restored: ImagePdqOutput = serde_json::from_str(&json_str).unwrap();
        assert_eq!(output, restored);
    }

    // ── §3.2 video-vpdq 出力 ──

    #[test]
    fn video_vpdq_output_from_spec() {
        let json = r#"{
            "frame_hashes": [
                { "frame": 0, "timestamp_ms": 0, "pdqhash": "hash0", "quality": 90 },
                { "frame": 1, "timestamp_ms": 1000, "pdqhash": "hash1", "quality": 88 }
            ]
        }"#;
        let output: VideoVpdqOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.frame_hashes.len(), 2);
        assert_eq!(output.frame_hashes[0].frame, 0);
        assert_eq!(output.frame_hashes[1].timestamp_ms, 1000);
    }

    // ── §3.2 cert-* 出力 ──

    #[test]
    fn cert_verify_output_from_spec() {
        let json = r#"{
            "verified": true,
            "chain": [
                { "subject": "Google Pixel 10", "issuer": "Google C2PA ICA G3" },
                { "subject": "Google C2PA ICA G3", "issuer": "Google C2PA Root CA G3" }
            ]
        }"#;
        let output: CertVerifyOutput = serde_json::from_str(json).unwrap();
        assert!(output.verified);
        assert_eq!(output.chain.len(), 2);
        assert_eq!(output.chain[0].subject, "Google Pixel 10");
    }

    #[test]
    fn cert_verify_output_not_verified() {
        let output = CertVerifyOutput {
            verified: false,
            chain: vec![],
        };
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["verified"], false);
    }
}
