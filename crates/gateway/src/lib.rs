// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Gateway
//!
//! Gateway API types and HTTP server.
//!
//! Spec §1.7, §2.5, §5.3
//!
//! ## Role
//!
//! The Gateway is a thin management layer between the client and TEE:
//! - Client authentication (API key, §5.3)
//! - Rate limiting (§5.3)
//! - TEE encryption public key caching and delivery (GET /keys, §2.5)
//! - Processor list caching and delivery (GET /processors, §2.5)
//! - Request relay to TEE (POST /process, §2.5)
//! - TEE health monitoring (GET /health, §2.5)
//! - TEE restart detection and key refresh (§5.3)
//! - Solana Extension endpoints (GET /solana-keys, POST /extension/solana, §6.2)
//!
//! ## Legacy
//!
//! `legacy/v0.1.0/crates/gateway/` -- Previous Gateway implementation (Axum).

pub mod auth;
pub mod endpoints;
pub mod error;
pub mod rate_limit;
pub mod server;
pub mod state;
pub mod tee_client;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// GET /keys (§2.5)
// ---------------------------------------------------------------------------

/// GET /keys レスポンス。
/// 仕様書 §2.5
///
/// TEEが現在保持している暗号化用公開鍵の一覧。
/// TEE再起動時に鍵は更新される。
///
/// # JSON例
/// ```json
/// {
///   "keys": {
///     "x25519": "(Base64公開鍵)",
///     "p256": "(Base64公開鍵)",
///     "ml-kem-768": "(Base64公開鍵)"
///   }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeysResponse {
    /// スイート名 → Base64エンコードされた公開鍵のマップ。
    pub keys: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// GET /processors (§2.5)
// ---------------------------------------------------------------------------

/// GET /processors レスポンス。
/// 仕様書 §2.5
///
/// TEEが対応しているprocessorのID一覧。
///
/// # JSON例
/// ```json
/// {
///   "processors": ["c2pa-verify", "image-pdq", "provenance-graph"]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessorsResponse {
    /// 対応processor IDの一覧。
    pub processors: Vec<String>,
}

// ---------------------------------------------------------------------------
// GET /health (§2.5)
// ---------------------------------------------------------------------------

/// GET /health レスポンス。
/// 仕様書 §2.5
///
/// TEEの稼働状態。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// 稼働状態。`"ok"` であればリクエスト受付可能。
    pub status: String,

    /// TEEベンダー種別。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tee_type: Option<String>,
}

// ---------------------------------------------------------------------------
// GET /solana-keys (§2.5)
// ---------------------------------------------------------------------------

/// GET /solana-keys レスポンス。
/// 仕様書 §2.5
///
/// Solana Extension用の公開鍵情報（Extension有効時のみ）。
///
/// # JSON例
/// ```json
/// {
///   "solana_pubkey": "(Base58公開鍵)"
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolanaKeysResponse {
    /// Solana Ed25519公開鍵（Base58エンコード）。
    pub solana_pubkey: String,
}

// ---------------------------------------------------------------------------
// POST /extension/solana (§2.5, §6.2)
// ---------------------------------------------------------------------------

/// POST /extension/solana リクエスト。
/// 仕様書 §6.2
///
/// コア処理の成果物をSolana cNFTとして発行するリクエスト。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolanaExtensionRequest {
    /// コア処理結果のオフチェーンデータURL。
    pub offchain_data_url: String,

    /// Payer公開鍵（Base58）。Fee payer かつ leaf_owner。
    pub payer: String,

    /// Merkle Treeアドレス（Base58）。
    pub merkle_tree: String,

    /// 最新のBlockhash（Base58）。
    pub recent_blockhash: String,

    /// コレクションアドレス（Base58、任意）。
    /// 開発者が選択するもので、信頼モデルの一部ではない。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
}

/// POST /extension/solana レスポンス。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolanaExtensionResponse {
    /// Base64エンコードされた部分署名済みトランザクション。
    pub partial_tx: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── §2.5 GET /keys ──

    #[test]
    fn keys_response_from_spec() {
        let json = r#"{
            "keys": {
                "x25519": "base64key1",
                "p256": "base64key2",
                "ml-kem-768": "base64key3"
            }
        }"#;
        let resp: KeysResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.keys.len(), 3);
        assert_eq!(resp.keys["x25519"], "base64key1");
    }

    #[test]
    fn keys_response_roundtrip() {
        let resp = KeysResponse {
            keys: {
                let mut m = HashMap::new();
                m.insert("x25519".into(), "key1".into());
                m.insert("p256".into(), "key2".into());
                m
            },
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let restored: KeysResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(resp, restored);
    }

    // ── §2.5 GET /processors ──

    #[test]
    fn processors_response_from_spec() {
        let json = r#"{
            "processors": ["c2pa-verify", "image-pdq", "provenance-graph"]
        }"#;
        let resp: ProcessorsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.processors.len(), 3);
        assert!(resp.processors.contains(&"c2pa-verify".into()));
    }

    // ── §2.5 GET /health ──

    #[test]
    fn health_response_roundtrip() {
        let resp = HealthResponse {
            status: "ok".into(),
            tee_type: Some("aws_nitro".into()),
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let restored: HealthResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(resp, restored);
    }

    #[test]
    fn health_response_tee_type_optional() {
        let resp = HealthResponse {
            status: "ok".into(),
            tee_type: None,
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        assert!(!json_str.contains("tee_type"));
    }

    // ── §6.2 Solana Extension ──

    #[test]
    fn solana_extension_request_from_spec() {
        let json = r#"{
            "offchain_data_url": "https://r2.example.com/output/abc123.json",
            "payer": "Base58Payer",
            "merkle_tree": "Base58MerkleTree",
            "recent_blockhash": "Base58Blockhash",
            "collection": "Base58Collection"
        }"#;
        let req: SolanaExtensionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.offchain_data_url,
            "https://r2.example.com/output/abc123.json"
        );
        assert_eq!(req.payer, "Base58Payer");
        assert_eq!(req.collection, Some("Base58Collection".into()));
    }

    #[test]
    fn solana_extension_request_without_collection() {
        let json = r#"{
            "offchain_data_url": "https://r2.example.com/output/abc123.json",
            "payer": "Base58Payer",
            "merkle_tree": "Base58MerkleTree",
            "recent_blockhash": "Base58Blockhash"
        }"#;
        let req: SolanaExtensionRequest = serde_json::from_str(json).unwrap();
        assert!(req.collection.is_none());
    }

    #[test]
    fn solana_extension_response_roundtrip() {
        let resp = SolanaExtensionResponse {
            partial_tx: "base64tx".into(),
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let restored: SolanaExtensionResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(resp, restored);
    }
}
