// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol 共有型定義
//!
//! 仕様書セクション5で定義されるデータ構造をRust構造体として提供する。
//!
//! ## エンコーディング規則
//! - Base58: Solanaアドレス、公開鍵（人間が読みやすく、紛らわしい文字を除外）
//! - Base64: バイナリデータ（暗号文、署名等）

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// signed_json 構造 (仕様書 §5.1 Step 4, Step 5)
// ---------------------------------------------------------------------------

/// TEEが検証結果をJSON形式でまとめ、自身の秘密鍵で署名したデータオブジェクト。
/// 仕様書 §5.1 Step 4 / Step 5
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedJson {
    /// 外殻（プロトコル識別・TEE情報・署名）
    #[serde(flatten)]
    pub core: SignedJsonCore,
    /// ペイロード（検証結果の本体）
    pub payload: serde_json::Value,
    /// cNFTオンチェーンメタデータ用属性
    pub attributes: Vec<Attribute>,
}

/// signed_jsonの外殻部分。CoreとExtensionで共通。
/// 仕様書 §5.1 Step 4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedJsonCore {
    /// プロトコル識別子 ("Title-v1" or "Title-Extension-v1")
    pub protocol: String,
    /// TEE種別 ("aws_nitro", "amd_sev_snp", "intel_tdx")
    pub tee_type: String,
    /// Base58エンコードされたEd25519公開鍵
    pub tee_pubkey: String,
    /// Base64エンコードされた署名（payload + attributesが対象）
    pub tee_signature: String,
    /// Base64エンコードされたAttestation Document
    pub tee_attestation: String,
}

// ---------------------------------------------------------------------------
// Payload 構造 (仕様書 §5.1 Step 4)
// ---------------------------------------------------------------------------

/// Core用ペイロード。来歴グラフを含む。
/// 仕様書 §5.1 Step 4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePayload {
    /// コンテンツハッシュ (SHA-256(Active Manifestの署名))
    pub content_hash: String,
    /// コンテンツのMIMEタイプ
    pub content_type: String,
    /// Base58エンコードされたウォレットアドレス
    pub creator_wallet: String,
    /// C2PA TSAタイムスタンプ（存在する場合）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tsa_timestamp: Option<u64>,
    /// TSA公開鍵のSHA-256ハッシュ（存在する場合）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tsa_pubkey_hash: Option<String>,
    /// Base64エンコードされたRFC 3161トークン（存在する場合）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tsa_token_data: Option<String>,
    /// 来歴グラフのノード一覧
    pub nodes: Vec<GraphNode>,
    /// 来歴グラフのリンク一覧
    pub links: Vec<GraphLink>,
}

/// Extension用ペイロード。WASM実行結果を含む。
/// 仕様書 §5.1 Step 5
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionPayload {
    /// コンテンツハッシュ
    pub content_hash: String,
    /// コンテンツのMIMEタイプ
    pub content_type: String,
    /// Base58エンコードされたウォレットアドレス
    pub creator_wallet: String,
    /// Extension識別子
    pub extension_id: String,
    /// WASMバイナリのArweave URI
    pub wasm_source: String,
    /// WASMバイナリのSHA-256ハッシュ
    pub wasm_hash: String,
    /// extension_inputs[extension_id]のSHA-256ハッシュ（Optional）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_input_hash: Option<String>,
    /// WASM実行結果（Extension固有のフィールド）
    #[serde(flatten)]
    pub result: serde_json::Value,
}

// ---------------------------------------------------------------------------
// 来歴グラフ (仕様書 §2.2, §5.1 Step 4)
// ---------------------------------------------------------------------------

/// 来歴グラフのノード。content_hashで識別される。
/// 仕様書 §2.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// コンテンツハッシュ (ノードID)
    pub id: String,
    /// ノードタイプ ("final" or "ingredient")
    #[serde(rename = "type")]
    pub node_type: String,
}

/// 来歴グラフのリンク。素材→派生の関係を表すエッジ。
/// 仕様書 §2.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphLink {
    /// 素材のcontent_hash
    pub source: String,
    /// 派生物のcontent_hash
    pub target: String,
    /// 関係の種類 (例: "audio", "image")
    pub role: String,
}

// ---------------------------------------------------------------------------
// Global Config (仕様書 §5.2 Step 1)
// ---------------------------------------------------------------------------

/// ブロックチェーン上のGlobal Config PDA。信頼の原点。
/// 仕様書 §5.2 Step 1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// DAO multi-sigのウォレットアドレス (Base58)
    pub authority: String,
    /// Core cNFTの公式コレクションMintアドレス (Base58)
    pub core_collection_mint: String,
    /// Extension cNFTの公式コレクションMintアドレス (Base58)
    pub ext_collection_mint: String,
    /// 信頼されたTEEノードのリスト
    pub trusted_tee_nodes: Vec<TrustedTeeNode>,
    /// 信頼するTSA公開鍵ハッシュのリスト (Base64)
    pub trusted_tsa_keys: Vec<String>,
    /// 信頼されたWASMモジュールのリスト
    pub trusted_wasm_modules: Vec<TrustedWasmModule>,
}

/// 信頼されたTEEノード情報。
/// 仕様書 §5.2 Step 1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedTeeNode {
    /// Base58エンコードされたEd25519署名用公開鍵
    pub signing_pubkey: String,
    /// Base64エンコードされたX25519暗号化用公開鍵（32バイト）
    pub encryption_pubkey: String,
    /// 暗号化アルゴリズム識別子
    pub encryption_algorithm: String,
    /// Base58エンコードされたGateway署名用Ed25519公開鍵
    pub gateway_pubkey: String,
    /// GatewayのHTTPエンドポイント
    pub gateway_endpoint: String,
    /// ノードステータス ("Active" 等)
    pub status: String,
    /// TEE種別 ("aws_nitro", "amd_sev_snp", "intel_tdx")
    pub tee_type: String,
    /// TEEインスタンスの期待される測定値
    pub expected_measurements: ExpectedMeasurements,
}

/// TEEの期待される測定値。tee_typeに応じてキー名が異なる。
/// 仕様書 §5.2 Step 1
///
/// キー名の慣例:
/// | tee_type | キー | 説明 |
/// |----------|------|------|
/// | `aws_nitro` | `PCR0`, `PCR1`, `PCR2` | Platform Configuration Registers |
/// | `amd_sev_snp` | `MEASUREMENT` | Guest VM initial state hash |
/// | `intel_tdx` | `MRTD`, `RTMR0`〜`RTMR3` | TD measurement registers |
pub type ExpectedMeasurements = HashMap<String, String>;

/// 信頼されたWASMモジュール情報。
/// 仕様書 §5.2 Step 1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedWasmModule {
    /// Extension識別子 (例: "phash-v1")
    pub extension_id: String,
    /// WASMバイナリのArweave URI
    pub wasm_source: String,
    /// WASMバイナリのSHA-256ハッシュ
    pub wasm_hash: String,
}

// ---------------------------------------------------------------------------
// 暗号化ペイロード (仕様書 §5.1 Step 2)
// ---------------------------------------------------------------------------

/// 暗号化されたペイロード。Temporary Storageに保存される。
/// 仕様書 §5.1 Step 2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// Base64エンコードされたX25519公開鍵（32バイト）
    pub ephemeral_pubkey: String,
    /// Base64エンコードされたAES-GCM nonce（12バイト）
    pub nonce: String,
    /// Base64エンコードされた暗号文
    pub ciphertext: String,
}

/// クライアントが構築するペイロード（暗号化前）。
/// 仕様書 §5.1 Step 1
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientPayload {
    /// Base58エンコードされたSolanaウォレットアドレス
    pub owner_wallet: String,
    /// Base64エンコードされたコンテンツバイナリ
    pub content: String,
    /// Base64エンコードされた.c2paファイル（Optional）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecar_manifest: Option<String>,
    /// Extension補助入力（Optional）。キーはextension_id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_inputs: Option<serde_json::Map<String, serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// API リクエスト/レスポンス (仕様書 §6.2, §5.1)
// ---------------------------------------------------------------------------

/// /verify リクエスト。
/// 仕様書 §5.1 Step 3
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyRequest {
    /// Temporary Storage上の暗号化ペイロードのURL
    pub download_url: String,
    /// 実行する検証の識別子リスト
    pub processor_ids: Vec<String>,
}

/// /verify レスポンス（復号後）。
/// 仕様書 §5.1 Step 6
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyResponse {
    /// processor_idごとのsigned_json
    pub results: Vec<ProcessorResult>,
}

/// 個別のプロセッサ結果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessorResult {
    /// プロセッサ識別子
    pub processor_id: String,
    /// TEEが生成したsigned_json
    pub signed_json: serde_json::Value,
}

/// /sign リクエスト。
/// 仕様書 §5.1 Step 8
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignRequest {
    /// Base58エンコードされたBlockhash
    pub recent_blockhash: String,
    /// 署名リクエストの一覧
    pub requests: Vec<SignRequestItem>,
}

/// /sign リクエストの個別アイテム。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignRequestItem {
    /// オフチェーンストレージのURI
    pub signed_json_uri: String,
}

/// /sign レスポンス。
/// 仕様書 §5.1 Step 10
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignResponse {
    /// Base64エンコードされた部分署名済みトランザクション
    pub partial_txs: Vec<String>,
}

// ---------------------------------------------------------------------------
// Gateway認証 (仕様書 §6.2)
// ---------------------------------------------------------------------------

/// Gateway認証の署名対象構造体。
/// GatewayAuthWrapperからgateway_signatureを除いた構造。
/// Gateway側で署名対象を構築し、TEE側で検証時に同一構造を再構築する。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayAuthSignTarget {
    /// HTTPメソッド
    pub method: String,
    /// リクエストパス
    pub path: String,
    /// クライアントのリクエスト本文
    pub body: serde_json::Value,
    /// リソース制限（Optional）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_limits: Option<ResourceLimits>,
}

/// Gateway認証ラッパー。GatewayがTEEに送信するリクエストの構造。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayAuthWrapper {
    /// HTTPメソッド
    pub method: String,
    /// リクエストパス
    pub path: String,
    /// クライアントのリクエスト本文
    pub body: serde_json::Value,
    /// リソース制限（Optional）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_limits: Option<ResourceLimits>,
    /// Base64エンコードされたEd25519署名
    pub gateway_signature: String,
}

/// Gatewayがリクエストごとに指定するリソース制限。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// 単体コンテンツの最大サイズ（バイト）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_single_content_bytes: Option<u64>,
    /// 同時処理可能な合計データ量（バイト）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_bytes: Option<u64>,
    /// 動的タイムアウト計算に使用する最低転送速度（バイト/秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_upload_speed_bytes: Option<u64>,
    /// 接続確立や検証開始にかかる固定オーバーヘッド時間（秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_processing_time_sec: Option<u64>,
    /// 処理を強制終了する絶対的な最大時間（秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_global_timeout_sec: Option<u64>,
    /// 次のデータチャンクが到着するまでの最大待機時間（秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_read_timeout_sec: Option<u64>,
    /// C2PAマニフェストグラフの最大サイズ（ノード+エッジ）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c2pa_max_graph_size: Option<u64>,
}

// ---------------------------------------------------------------------------
// cNFT メタデータ (仕様書 §5.1 Step 11)
// ---------------------------------------------------------------------------

/// cNFTオンチェーンメタデータ。
/// 仕様書 §5.1 Step 11
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CnftMetadata {
    /// トークン名 (例: "Title #0x1234abcd")
    pub name: String,
    /// シンボル ("TITLE" for Core, Extension種別名 for Extension)
    pub symbol: String,
    /// オフチェーンデータのURI
    pub uri: String,
    /// 属性の一覧
    pub attributes: Vec<Attribute>,
}

/// Metaplex標準の属性（trait_type + value）。
/// 仕様書 §5.1 Step 4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    /// 属性の種類
    pub trait_type: String,
    /// 属性の値
    pub value: String,
}

// ---------------------------------------------------------------------------
// /verify 暗号化レスポンス (仕様書 §5.1 Step 6)
// ---------------------------------------------------------------------------

/// /verifyの暗号化レスポンス（Gateway経由で返却）。
/// 仕様書 §5.1 Step 6
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedResponse {
    /// Base64エンコードされたAES-GCM nonce（12バイト）
    pub nonce: String,
    /// Base64エンコードされた暗号文
    pub ciphertext: String,
}

// ---------------------------------------------------------------------------
// /sign-and-mint レスポンス (仕様書 §6.2)
// ---------------------------------------------------------------------------

/// /sign-and-mint レスポンス。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignAndMintResponse {
    /// ブロードキャスト済みトランザクションの署名
    pub tx_signatures: Vec<String>,
}

// ---------------------------------------------------------------------------
// /upload-url (仕様書 §6.2)
// ---------------------------------------------------------------------------

/// /upload-url リクエスト。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadUrlRequest {
    /// コンテンツサイズ（バイト）
    pub content_size: u64,
    /// コンテンツのMIMEタイプ
    pub content_type: String,
}

/// /upload-url レスポンス。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadUrlResponse {
    /// 署名付きアップロードURL
    pub upload_url: String,
    /// TEEがアクセスするためのURL
    pub download_url: String,
    /// URL有効期限（UNIXタイムスタンプ）
    pub expires_at: u64,
}

// ---------------------------------------------------------------------------
// /create-tree (仕様書 §6.4)
// ---------------------------------------------------------------------------

/// /create-tree リクエスト。
/// 仕様書 §6.4
///
/// payerはTEE内部の署名鍵が兼ねる（TEEウォレットに事前にSOLを送金する必要がある）。
/// これにより、Merkle Treeの作成・操作権限が完全にTEE内部に閉じる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTreeRequest {
    /// Merkle Treeの深さ
    pub max_depth: u32,
    /// 最大バッファサイズ
    pub max_buffer_size: u32,
    /// Base58エンコードされたBlockhash
    pub recent_blockhash: String,
}

/// /create-tree レスポンス。
/// 仕様書 §6.4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTreeResponse {
    /// Base64エンコードされた完全署名済みトランザクション（そのままブロードキャスト可能）
    pub signed_tx: String,
    /// Base58エンコードされたMerkle Treeアドレス
    pub tree_address: String,
    /// Base58エンコードされたEd25519署名用公開鍵
    pub signing_pubkey: String,
    /// Base64エンコードされたX25519暗号化用公開鍵
    pub encryption_pubkey: String,
}

/// /register-node リクエスト。
/// 仕様書 §8.2
///
/// TEE内部でregister_tee_node命令のトランザクションを構築・署名する。
/// TEEのsigning_pubkeyがpayerとなり、鍵所有を暗号的に証明する。
/// authorityの共同署名が必要な部分署名済みトランザクションを返す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterNodeRequest {
    /// GatewayエンドポイントURL（オンチェーンに記録される）
    pub gateway_endpoint: String,
    /// Base58エンコードされたGateway署名用Ed25519公開鍵
    pub gateway_pubkey: String,
    /// Base58エンコードされたBlockhash
    pub recent_blockhash: String,
    /// Base58エンコードされたDAO authority公開鍵
    pub authority: String,
    /// Base58エンコードされたtitle-configプログラムID
    pub program_id: String,
}

/// /register-node レスポンス。
/// 仕様書 §8.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterNodeResponse {
    /// Base64エンコードされた部分署名済みトランザクション（TEE署名済み、authority署名が必要）
    pub partial_tx: String,
    /// Base58エンコードされたTEE Ed25519署名用公開鍵
    pub signing_pubkey: String,
    /// Base64エンコードされたTEE X25519暗号化用公開鍵
    pub encryption_pubkey: String,
    /// Base58エンコードされたTeeNodeAccount PDAアドレス
    pub tee_node_pda: String,
}

// ---------------------------------------------------------------------------
// ノード情報 (仕様書 §6.2)
// ---------------------------------------------------------------------------

/// /.well-known/title-node-info レスポンス。
/// 仕様書 §6.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Base58エンコードされたEd25519署名用公開鍵
    pub signing_pubkey: String,
    /// サポートするExtensionの識別子リスト
    pub supported_extensions: Vec<String>,
    /// リソース制限情報
    pub limits: NodeLimits,
}

/// ノードのリソース制限情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeLimits {
    /// 単体コンテンツの最大サイズ（バイト）
    pub max_single_content_bytes: u64,
    /// 同時処理可能な合計データ量（バイト）
    pub max_concurrent_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // #[serde(flatten)] — SignedJson.core のフィールドがトップレベルに展開されるか
    // -----------------------------------------------------------------------

    #[test]
    fn test_signed_json_flatten_core_fields() {
        let sj = SignedJson {
            core: SignedJsonCore {
                protocol: "Title-v1".into(),
                tee_type: "mock".into(),
                tee_pubkey: "pk".into(),
                tee_signature: "sig".into(),
                tee_attestation: "att".into(),
            },
            payload: serde_json::json!({"key": "val"}),
            attributes: vec![],
        };
        let json = serde_json::to_value(&sj).unwrap();
        // flatten により protocol 等がトップレベルに存在
        assert_eq!(json["protocol"], "Title-v1");
        assert_eq!(json["tee_type"], "mock");
        assert_eq!(json["tee_pubkey"], "pk");
        assert_eq!(json["tee_signature"], "sig");
        assert_eq!(json["tee_attestation"], "att");
        assert_eq!(json["payload"]["key"], "val");
        // "core" キーは存在しない（flattenされているため）
        assert!(json.get("core").is_none());
    }

    #[test]
    fn test_signed_json_flatten_roundtrip() {
        let original = SignedJson {
            core: SignedJsonCore {
                protocol: "Title-v1".into(),
                tee_type: "mock".into(),
                tee_pubkey: "pk".into(),
                tee_signature: "sig".into(),
                tee_attestation: "att".into(),
            },
            payload: serde_json::json!({"n": 42}),
            attributes: vec![Attribute {
                trait_type: "protocol".into(),
                value: "Title-v1".into(),
            }],
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let restored: SignedJson = serde_json::from_str(&json_str).unwrap();
        assert_eq!(original, restored);
    }

    // -----------------------------------------------------------------------
    // #[serde(flatten)] — ExtensionPayload.result のフィールドがマージされるか
    // -----------------------------------------------------------------------

    #[test]
    fn test_extension_payload_flatten_result() {
        let ep = ExtensionPayload {
            content_hash: "0xabc".into(),
            content_type: "image/png".into(),
            creator_wallet: "wallet".into(),
            extension_id: "phash-v1".into(),
            wasm_source: "arweave://xxx".into(),
            wasm_hash: "0xdef".into(),
            extension_input_hash: None,
            result: serde_json::json!({"phash": "abcd1234", "confidence": 0.95}),
        };
        let json = serde_json::to_value(&ep).unwrap();
        // result のフィールドがトップレベルにマージ
        assert_eq!(json["phash"], "abcd1234");
        assert!(json.get("result").is_none());
        // 通常フィールドも存在
        assert_eq!(json["extension_id"], "phash-v1");
    }

    // -----------------------------------------------------------------------
    // #[serde(rename = "type")] — GraphNode.node_type → "type"
    // -----------------------------------------------------------------------

    #[test]
    fn test_graph_node_rename_type() {
        let node = GraphNode {
            id: "0x1234".into(),
            node_type: "final".into(),
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["type"], "final");
        assert!(json.get("node_type").is_none());
    }

    #[test]
    fn test_graph_node_rename_roundtrip() {
        let original = GraphNode {
            id: "0xabcd".into(),
            node_type: "ingredient".into(),
        };
        let json_str = serde_json::to_string(&original).unwrap();
        assert!(json_str.contains("\"type\""));
        assert!(!json_str.contains("\"node_type\""));
        let restored: GraphNode = serde_json::from_str(&json_str).unwrap();
        assert_eq!(original, restored);
    }

    // -----------------------------------------------------------------------
    // #[serde(skip_serializing_if = "Option::is_none")] — None時にフィールド省略
    // -----------------------------------------------------------------------

    #[test]
    fn test_core_payload_skip_none_fields() {
        let payload = CorePayload {
            content_hash: "0x1".into(),
            content_type: "image/jpeg".into(),
            creator_wallet: "w".into(),
            tsa_timestamp: None,
            tsa_pubkey_hash: None,
            tsa_token_data: None,
            nodes: vec![],
            links: vec![],
        };
        let json_str = serde_json::to_string(&payload).unwrap();
        assert!(!json_str.contains("tsa_timestamp"));
        assert!(!json_str.contains("tsa_pubkey_hash"));
        assert!(!json_str.contains("tsa_token_data"));
    }

    #[test]
    fn test_core_payload_includes_some_fields() {
        let payload = CorePayload {
            content_hash: "0x1".into(),
            content_type: "image/jpeg".into(),
            creator_wallet: "w".into(),
            tsa_timestamp: Some(1700000000),
            tsa_pubkey_hash: Some("hash".into()),
            tsa_token_data: Some("dG9rZW4=".into()),
            nodes: vec![],
            links: vec![],
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["tsa_timestamp"], 1700000000);
        assert_eq!(json["tsa_pubkey_hash"], "hash");
        assert_eq!(json["tsa_token_data"], "dG9rZW4=");
    }

    #[test]
    fn test_resource_limits_skip_all_none() {
        let limits = ResourceLimits {
            max_single_content_bytes: None,
            max_concurrent_bytes: None,
            min_upload_speed_bytes: None,
            base_processing_time_sec: None,
            max_global_timeout_sec: None,
            chunk_read_timeout_sec: None,
            c2pa_max_graph_size: None,
        };
        let json_str = serde_json::to_string(&limits).unwrap();
        assert_eq!(json_str, "{}");
    }

    // -----------------------------------------------------------------------
    // Roundtrip テスト — 代表的な型
    // -----------------------------------------------------------------------

    #[test]
    fn test_attribute_roundtrip() {
        let attr = Attribute {
            trait_type: "content_hash".into(),
            value: "0xdeadbeef".into(),
        };
        let json_str = serde_json::to_string(&attr).unwrap();
        let restored: Attribute = serde_json::from_str(&json_str).unwrap();
        assert_eq!(attr, restored);
    }

    #[test]
    fn test_graph_link_roundtrip() {
        let link = GraphLink {
            source: "0xaaa".into(),
            target: "0xbbb".into(),
            role: "image".into(),
        };
        let json_str = serde_json::to_string(&link).unwrap();
        let restored: GraphLink = serde_json::from_str(&json_str).unwrap();
        assert_eq!(link, restored);
    }

    #[test]
    fn test_verify_request_roundtrip() {
        let req = VerifyRequest {
            download_url: "https://example.com/data".into(),
            processor_ids: vec!["core".into(), "phash-v1".into()],
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let restored: VerifyRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(req, restored);
    }

    #[test]
    fn test_encrypted_payload_roundtrip() {
        let ep = EncryptedPayload {
            ephemeral_pubkey: "cGsi".into(),
            nonce: "bm9uY2U=".into(),
            ciphertext: "Y2lwaGVy".into(),
        };
        let json_str = serde_json::to_string(&ep).unwrap();
        let restored: EncryptedPayload = serde_json::from_str(&json_str).unwrap();
        assert_eq!(ep, restored);
    }
}
