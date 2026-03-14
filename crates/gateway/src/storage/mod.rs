// SPDX-License-Identifier: Apache-2.0

//! # Temporary Storage
//!
//! 仕様書 §6.3
//!
//! Gateway運用者が選択可能なTemporary Storageの抽象インターフェース。

#[cfg(feature = "vendor-aws")]
pub mod s3;

#[cfg(feature = "vendor-aws")]
pub use s3::{S3TempStorage, S3SignedJsonStorage};

#[cfg(feature = "vendor-irys")]
pub mod irys;

#[cfg(feature = "vendor-irys")]
pub use irys::IrysSignedJsonStorage;

#[cfg(feature = "vendor-local")]
pub mod local;

#[cfg(feature = "vendor-local")]
pub use local::LocalTempStorage;

use crate::error::GatewayError;

/// Temporary Storageの署名付きURL生成結果。
/// 仕様書 §6.3
pub struct PresignedUrls {
    /// クライアントがアップロードに使用するURL（PUT）
    pub upload_url: String,
    /// TEEがダウンロードに使用するURL（GET）
    pub download_url: String,
}

/// Temporary Storageの抽象インターフェース。
/// 仕様書 §6.3
///
/// Gateway運用者は任意のストレージバックエンドを実装として選択できる。
#[async_trait::async_trait]
pub trait TempStorage: Send + Sync {
    /// 署名付きアップロードURL（PUT）とダウンロードURL（GET）を生成する。
    ///
    /// - `upload_url`: クライアントが暗号化ペイロードをアップロードするために使用
    /// - `download_url`: TEEが暗号化ペイロードをフェッチするために使用
    ///
    /// upload_urlとdownload_urlが異なるエンドポイントを指す場合がある
    /// （例: Docker内部ホスト名 vs 外部ホスト名）。
    async fn generate_presigned_urls(
        &self,
        object_key: &str,
        expiry_secs: u32,
    ) -> Result<PresignedUrls, GatewayError>;
}

/// signed_jsonのストレージインターフェース。
///
/// Gatewayがsigned_jsonの保存を代行する場合に使用する。
/// ノード運営者のオプション機能であり、`/health` の capabilities で公開される。
///
/// 保存先の永続性は実装に依存する（S3: バケットのライフサイクル設定次第、
/// Arweave: 永続保証）。cNFTのメタデータURIとして使用されるため、
/// ノード運営者は適切な保持期間を保証する責任を負う。
#[async_trait::async_trait]
pub trait SignedJsonStorage: Send + Sync {
    /// signed_jsonを保存し、アクセス可能なURIを返す。
    async fn store(&self, key: &str, data: &[u8]) -> Result<String, GatewayError>;
}

// ---------------------------------------------------------------------------
// SignedJsonStorageRouter
// ---------------------------------------------------------------------------

/// signed_jsonストレージのルーター。
///
/// processor_idに応じて異なるストレージバックエンドにルーティングする。
/// SDKと同様、`core-c2pa` も extension も全て processor_id で統一的に扱う。
/// ルーティング戦略はノード運営者の裁量であり、プロトコルレベルの強要ではない。
///
/// processor_idの取得: `payload.extension_id` があればそれを使用、なければ `"core-c2pa"`。
pub struct SignedJsonStorageRouter {
    /// processor_id → ストレージのマッピング。
    /// 例: `"core-c2pa"` → Irys, `"phash-v1"` → S3
    routes: std::collections::HashMap<String, Box<dyn SignedJsonStorage>>,
    /// マッピングにないprocessor_id用のデフォルトストレージ。
    default_storage: Option<Box<dyn SignedJsonStorage>>,
}

impl SignedJsonStorageRouter {
    /// ルーターを構築する。
    ///
    /// `routes` と `default_storage` の両方が空の場合は `None` を返す（機能無効）。
    pub fn new(
        routes: std::collections::HashMap<String, Box<dyn SignedJsonStorage>>,
        default_storage: Option<Box<dyn SignedJsonStorage>>,
    ) -> Option<Self> {
        if routes.is_empty() && default_storage.is_none() {
            return None;
        }
        Some(Self {
            routes,
            default_storage,
        })
    }

    /// signed_jsonからprocessor_idを取得し、適切なストレージに保存する。
    ///
    /// processor_idの判定:
    /// - `payload.extension_id` があれば → そのまま processor_id
    /// - なければ → `"core-c2pa"`
    ///
    /// ルーティング:
    /// 1. `routes` に processor_id のエントリがあれば → そのストレージ
    /// 2. なければ → `default_storage`
    pub async fn store(
        &self,
        signed_json: &serde_json::Value,
        key: &str,
        data: &[u8],
    ) -> Result<String, GatewayError> {
        let processor_id = signed_json
            .pointer("/payload/extension_id")
            .and_then(|v| v.as_str())
            .unwrap_or("core-c2pa");

        let storage = self
            .routes
            .get(processor_id)
            .or(self.default_storage.as_ref());

        let storage = storage.ok_or_else(|| {
            GatewayError::BadRequest(
                "このノードはsigned_json保存代行に対応していません。\
                 signed_json_uriを指定してください"
                    .to_string(),
            )
        })?;

        storage.store(key, data).await
    }
}
