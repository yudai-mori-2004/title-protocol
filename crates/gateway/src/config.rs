// SPDX-License-Identifier: Apache-2.0

//! # Gateway設定・共有状態
//!
//! 仕様書 §6.2
//!
//! 環境変数からの設定読み込みとGatewayの共有状態の定義。

use ed25519_dalek::SigningKey as Ed25519SigningKey;
use title_types::*;

use crate::storage::{SignedJsonStorage, TempStorage};

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
    /// Temporary Storage（トレイトで抽象化）
    /// 仕様書 §6.3
    pub temp_storage: Box<dyn TempStorage>,
    /// signed_jsonストレージ（オプション）。
    /// 設定されている場合、`/sign-and-mint` でsigned_json本体を受け取り保存を代行できる。
    pub signed_json_storage: Option<Box<dyn SignedJsonStorage>>,
    /// Solana RPC URL（sign-and-mint用）
    pub solana_rpc_url: Option<String>,
    /// Solana Gateway ウォレットキーペア（sign-and-mint用）
    pub solana_keypair: Option<solana_sdk::signer::keypair::Keypair>,
    /// デフォルトリソース制限（リクエストごと）
    /// オンチェーン値でクランプ済み。
    pub default_resource_limits: ResourceLimits,
    /// オンチェーンから取得したリソース制限（参照用・ログ出力用）
    #[allow(dead_code)]
    pub on_chain_resource_limits: Option<ResourceLimits>,
    /// アップロード最大サイズ（バイト）
    pub max_upload_size: u64,
    /// 署名付きURLの有効期限（秒）
    pub presign_expiry_secs: u32,
}
