//! # Temporary Storage
//!
//! 仕様書 §6.3
//!
//! Gateway運用者が選択可能なTemporary Storageの抽象インターフェース。
//! S3互換ストレージ実装は `s3` サブモジュールを参照。

#[cfg(feature = "vendor-aws")]
pub mod s3;

#[cfg(feature = "vendor-aws")]
pub use s3::S3TempStorage;

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
/// Gateway運用者はS3互換ストレージ（MinIO, AWS S3, Cloudflare R2等）や
/// その他のストレージバックエンドを実装として選択できる。
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
