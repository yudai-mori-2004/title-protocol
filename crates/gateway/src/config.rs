//! # Gateway設定・共有状態
//!
//! 仕様書 §6.2
//!
//! 環境変数からの設定読み込みとGatewayの共有状態の定義。

use ed25519_dalek::{SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey};
use title_types::*;

use crate::storage::TempStorage;

/// Gatewayの共有状態。
/// 仕様書 §6.2
pub struct GatewayState {
    /// TEEのエンドポイントURL
    pub tee_endpoint: String,
    /// HTTPクライアント
    pub http_client: reqwest::Client,
    /// Gateway認証用Ed25519秘密鍵
    /// 仕様書 §6.2: Gateway秘密鍵で署名
    pub signing_key: Ed25519SigningKey,
    /// Gateway認証用Ed25519公開鍵
    pub verifying_key: Ed25519VerifyingKey,
    /// Temporary Storage（S3互換等、トレイトで抽象化）
    /// 仕様書 §6.3
    pub temp_storage: Box<dyn TempStorage>,
    /// Solana RPC URL（sign-and-mint用）
    pub solana_rpc_url: Option<String>,
    /// Solana Gateway ウォレットキーペア（sign-and-mint用）
    pub solana_keypair: Option<solana_sdk::signer::keypair::Keypair>,
    /// サポートするExtensionリスト
    pub supported_extensions: Vec<String>,
    /// ノードのリソース制限情報
    pub node_limits: NodeLimits,
    /// デフォルトリソース制限（リクエストごと）
    pub default_resource_limits: ResourceLimits,
    /// アップロード最大サイズ（バイト）
    pub max_upload_size: u64,
    /// 署名付きURLの有効期限（秒）
    pub presign_expiry_secs: u32,
}
