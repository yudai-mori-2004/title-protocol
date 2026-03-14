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
