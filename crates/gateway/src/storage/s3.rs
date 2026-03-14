// SPDX-License-Identifier: Apache-2.0

//! # S3互換 Temporary Storage 実装
//!
//! 仕様書 §6.3
//!
//! AWS S3, MinIO, Cloudflare R2 等のS3互換APIを使用する
//! Temporary Storage実装。

use super::{PresignedUrls, SignedJsonStorage, TempStorage};
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
    pub(crate) fn init_bucket(
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

// ---------------------------------------------------------------------------
// signed_json ストレージ (S3互換)
// ---------------------------------------------------------------------------

/// S3互換ストレージによるsigned_jsonストレージ実装。
///
/// signed_jsonをS3バケットに保存し、パブリックURLを返す。
/// バケットのライフサイクル設定やアクセスポリシーはノード運営者の責任。
pub struct S3SignedJsonStorage {
    /// オブジェクト保存用バケット
    bucket: s3::Bucket,
    /// パブリックアクセス用ベースURL（例: `https://bucket.s3.region.amazonaws.com`）
    public_base_url: String,
}

impl S3SignedJsonStorage {
    /// 環境変数から構築する。
    ///
    /// `SIGNED_JSON_S3_BUCKET` が未設定の場合は `None` を返す（機能無効）。
    /// S3接続情報はTempStorageと共有する（`S3_ENDPOINT`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`）。
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let bucket_name = match std::env::var("SIGNED_JSON_S3_BUCKET") {
            Ok(name) if !name.is_empty() => name,
            _ => return Ok(None),
        };

        let endpoint = std::env::var("S3_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:9000".to_string());
        let access_key =
            std::env::var("S3_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let secret_key =
            std::env::var("S3_SECRET_KEY").unwrap_or_else(|_| "minioadmin".to_string());

        let bucket =
            S3TempStorage::init_bucket(&endpoint, &access_key, &secret_key, &bucket_name)?;

        // パブリックURL: 明示指定 > リージョンから自動構築
        let public_base_url = std::env::var("SIGNED_JSON_S3_PUBLIC_URL")
            .unwrap_or_else(|_| {
                let region = std::env::var("S3_REGION")
                    .unwrap_or_else(|_| "ap-northeast-1".to_string());
                format!("https://{bucket_name}.s3.{region}.amazonaws.com")
            });

        tracing::info!(
            bucket = %bucket_name,
            public_url = %public_base_url,
            "signed_jsonストレージを設定"
        );

        Ok(Some(Self {
            bucket,
            public_base_url,
        }))
    }
}

#[async_trait::async_trait]
impl SignedJsonStorage for S3SignedJsonStorage {
    /// signed_jsonをS3に保存し、パブリックURLを返す。
    async fn store(&self, key: &str, data: &[u8]) -> Result<String, GatewayError> {
        self.bucket
            .put_object_with_content_type(key, data, "application/json")
            .await
            .map_err(|e| GatewayError::Storage(format!("signed_json保存失敗: {e}")))?;

        Ok(format!("{}/{}", self.public_base_url, key))
    }
}
