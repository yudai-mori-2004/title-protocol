// SPDX-License-Identifier: Apache-2.0

//! # Irys (Arweave) signed_jsonストレージ実装
//!
//! Node.jsサイドカー（`@irys/upload`）を経由してArweaveに永続保存する。
//! Gateway は中継のみ行い、Irys SDK の処理はサイドカーに委譲する。

use super::SignedJsonStorage;
use crate::error::GatewayError;

/// Irys (Arweave) によるsigned_jsonストレージ実装。
///
/// Node.jsサイドカー（Irys Uploader）にHTTP POSTして、
/// `@irys/upload` 経由でArweaveにデータを永続保存する。
/// GatewayのSolanaキーペアをリクエストごとにサイドカーへ渡す。
pub struct IrysSignedJsonStorage {
    /// サイドカーのベースURL（例: `http://irys-uploader:3001`）
    uploader_url: String,
    /// GatewayのSolanaキーペア（Base58、サイドカーに署名用として渡す）
    solana_keypair_b58: String,
    /// HTTPクライアント
    http_client: reqwest::Client,
}

impl IrysSignedJsonStorage {
    /// 環境変数から構築する。
    ///
    /// `IRYS_UPLOADER_URL` が未設定の場合は `None` を返す（機能無効）。
    /// Solanaキーペアは `GATEWAY_SOLANA_KEYPAIR` から取得する。
    ///
    /// | 環境変数 | デフォルト | 説明 |
    /// |---------|----------|------|
    /// | `IRYS_UPLOADER_URL` | (必須) | Irysサイドカーのエンドポイント |
    /// | `GATEWAY_SOLANA_KEYPAIR` | (必須) | Solanaキーペア（Base58、Irys署名に使用） |
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let uploader_url = match std::env::var("IRYS_UPLOADER_URL") {
            Ok(url) if !url.is_empty() => url.trim_end_matches('/').to_string(),
            _ => return Ok(None),
        };

        let solana_keypair_b58 = std::env::var("GATEWAY_SOLANA_KEYPAIR")
            .map_err(|_| anyhow::anyhow!(
                "IRYS_UPLOADER_URL が設定されていますが GATEWAY_SOLANA_KEYPAIR が未設定です"
            ))?;

        tracing::info!(
            uploader_url = %uploader_url,
            "Irys signed_jsonストレージを設定（サイドカー経由）"
        );

        Ok(Some(Self {
            uploader_url,
            solana_keypair_b58,
            http_client: reqwest::Client::new(),
        }))
    }
}

#[async_trait::async_trait]
impl SignedJsonStorage for IrysSignedJsonStorage {
    /// signed_jsonをIrysサイドカー経由でArweaveにアップロードし、永続URIを返す。
    async fn store(&self, _key: &str, data: &[u8]) -> Result<String, GatewayError> {
        let body = serde_json::json!({
            "data": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data),
            "content_type": "application/json",
            "private_key": self.solana_keypair_b58,
        });

        let response = self
            .http_client
            .post(format!("{}/upload", self.uploader_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| GatewayError::Storage(format!("Irysサイドカーへの接続に失敗: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            return Err(GatewayError::Storage(format!(
                "Irysアップロード失敗: HTTP {status} - {text}"
            )));
        }

        let res_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| GatewayError::Storage(format!("Irysレスポンスのパースに失敗: {e}")))?;

        let url = res_body
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                GatewayError::Storage(format!(
                    "IrysレスポンスにURLがありません: {res_body}"
                ))
            })?;

        Ok(url.to_string())
    }
}
