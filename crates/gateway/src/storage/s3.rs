//! # S3互換 Temporary Storage 実装
//!
//! 仕様書 §6.3
//!
//! AWS S3, MinIO, Cloudflare R2 等のS3互換APIを使用する
//! Temporary Storage実装。

use super::{PresignedUrls, TempStorage};
use crate::error::GatewayError;

/// S3互換ストレージによるTemporary Storage実装。
/// AWS S3, MinIO, Cloudflare R2 等のS3互換APIを使用する。
/// 仕様書 §6.3
pub struct S3TempStorage {
    /// 内部通信用バケット（TEEからのダウンロード等）
    bucket_internal: s3::Bucket,
    /// クライアント向けバケット（署名付きURL生成用）。
    /// Docker内部ホスト名と外部ホスト名が異なる場合に使用。
    /// Noneの場合はbucket_internalを使用する。
    bucket_public: Option<s3::Bucket>,
}

impl S3TempStorage {
    /// S3互換バケットからTempStorageを構築する。
    /// 仕様書 §6.3
    pub fn new(
        bucket_internal: s3::Bucket,
        bucket_public: Option<s3::Bucket>,
    ) -> Self {
        Self {
            bucket_internal,
            bucket_public,
        }
    }

    /// 環境変数からS3互換バケットを初期化する。
    /// 仕様書 §6.3
    fn init_bucket(
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
        bucket_name: &str,
    ) -> anyhow::Result<s3::Bucket> {
        // AWS S3エンドポイント（s3.REGION.amazonaws.com）からリージョンを自動検出。
        // 非AWSエンドポイントではus-east-1をフォールバックとして使用。
        let detected_region = std::env::var("S3_REGION").ok().unwrap_or_else(|| {
            if let Some(caps) = endpoint.find("s3.").and_then(|start| {
                let rest = &endpoint[start + 3..];
                rest.find(".amazonaws.com").map(|end| rest[..end].to_string())
            }) {
                caps
            } else {
                "us-east-1".to_string()
            }
        });
        let region = s3::Region::Custom {
            region: detected_region,
            endpoint: endpoint.to_string(),
        };

        let credentials = s3::creds::Credentials::new(
            Some(access_key),
            Some(secret_key),
            None,
            None,
            None,
        )?;

        let bucket = s3::Bucket::new(bucket_name, region, credentials)?.with_path_style();

        Ok(*bucket)
    }

    /// 環境変数から構築する。
    /// 仕様書 §6.3
    pub fn from_env() -> anyhow::Result<Self> {
        let endpoint = std::env::var("S3_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:9000".to_string());
        let access_key =
            std::env::var("S3_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let secret_key =
            std::env::var("S3_SECRET_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let bucket_name =
            std::env::var("S3_BUCKET").unwrap_or_else(|_| "title-uploads".to_string());

        let bucket_internal =
            Self::init_bucket(&endpoint, &access_key, &secret_key, &bucket_name)?;

        let bucket_public = std::env::var("S3_PUBLIC_ENDPOINT")
            .ok()
            .map(|public_ep| {
                tracing::info!(
                    s3_public_endpoint = %public_ep,
                    "クライアント向けS3エンドポイントを設定"
                );
                Self::init_bucket(&public_ep, &access_key, &secret_key, &bucket_name)
            })
            .transpose()?;

        Ok(Self::new(bucket_internal, bucket_public))
    }
}

#[async_trait::async_trait]
impl TempStorage for S3TempStorage {
    /// 署名付きURLを生成する。
    /// 仕様書 §6.3
    async fn generate_presigned_urls(
        &self,
        object_key: &str,
        expiry_secs: u32,
    ) -> Result<PresignedUrls, GatewayError> {
        let public_bucket = self.bucket_public.as_ref().unwrap_or(&self.bucket_internal);

        let upload_url = public_bucket
            .presign_put(object_key, expiry_secs, None, None)
            .await
            .map_err(|e| GatewayError::Storage(format!("署名付きアップロードURL生成失敗: {e}")))?;

        let download_url = self
            .bucket_internal
            .presign_get(object_key, expiry_secs, None)
            .await
            .map_err(|e| {
                GatewayError::Storage(format!("署名付きダウンロードURL生成失敗: {e}"))
            })?;

        Ok(PresignedUrls {
            upload_url,
            download_url,
        })
    }
}
