// SPDX-License-Identifier: Apache-2.0

//! # ローカル Temporary Storage 実装
//!
//! 仕様書 §6.3
//!
//! `title-temp-storage` サーバー（独立プロセス）と連携する
//! Temporary Storage実装。素朴なHTTP PUT/GETでファイルを管理する。

use super::{PresignedUrls, TempStorage};
use crate::error::GatewayError;

/// ローカルファイルサーバーによるTemporary Storage実装。
/// `title-temp-storage` サーバーと連携する。
/// 仕様書 §6.3
pub struct LocalTempStorage {
    /// 内部通信用ベースURL（TEEからのダウンロード等）
    internal_base_url: String,
    /// クライアント向けベースURL。
    /// Docker内部ホスト名と外部ホスト名が異なる場合に使用。
    /// Noneの場合はinternal_base_urlを使用する。
    public_base_url: Option<String>,
}

impl LocalTempStorage {
    /// ベースURLからLocalTempStorageを構築する。
    /// 仕様書 §6.3
    pub fn new(internal_base_url: String, public_base_url: Option<String>) -> Self {
        Self {
            internal_base_url,
            public_base_url,
        }
    }

    /// 環境変数から構築する。
    /// 仕様書 §6.3
    ///
    /// - `LOCAL_STORAGE_ENDPOINT` — TEE向けベースURL（デフォルト: `http://localhost:3001`）
    /// - `LOCAL_STORAGE_PUBLIC_ENDPOINT` — クライアント向けベースURL（省略可）
    pub fn from_env() -> anyhow::Result<Self> {
        let internal_base_url = std::env::var("LOCAL_STORAGE_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:3001".to_string());

        let public_base_url = std::env::var("LOCAL_STORAGE_PUBLIC_ENDPOINT").ok();

        if let Some(ref public_ep) = public_base_url {
            tracing::info!(
                local_storage_public_endpoint = %public_ep,
                "クライアント向けローカルストレージエンドポイントを設定"
            );
        }

        Ok(Self::new(internal_base_url, public_base_url))
    }
}

#[async_trait::async_trait]
impl TempStorage for LocalTempStorage {
    /// ローカルTempStorageサーバーへのURLを生成する。
    /// 仕様書 §6.3
    ///
    /// 認証なしの素朴なURLを返す。ローカル開発専用。
    async fn generate_presigned_urls(
        &self,
        object_key: &str,
        _expiry_secs: u32,
    ) -> Result<PresignedUrls, GatewayError> {
        let public_base = self
            .public_base_url
            .as_deref()
            .unwrap_or(&self.internal_base_url);

        let upload_url = format!("{}/objects/{}", public_base, object_key);
        let download_url = format!("{}/objects/{}", self.internal_base_url, object_key);

        Ok(PresignedUrls {
            upload_url,
            download_url,
        })
    }
}
