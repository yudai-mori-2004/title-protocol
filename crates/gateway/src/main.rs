//! # Title Protocol Gateway
//!
//! 仕様書 §6.2
//!
//! ## 役割
//! - クライアント認証（APIキー管理）
//! - レート制限
//! - Temporary Storageへの署名付きURL発行
//! - リクエストごとのリソース制限の付与
//! - TEEへのリクエスト中継
//! - 代行ミント（オプション）
//!
//! ## API エンドポイント
//! - `POST /upload-url` — 署名付きURL発行
//! - `POST /verify` — TEEへのリクエスト中継 + Gateway認証署名付与
//! - `POST /sign` — TEEへのリクエスト中継
//! - `POST /sign-and-mint` — sign + ブロードキャスト代行
//! - `GET /.well-known/title-node-info` — ノード情報公開

mod auth;
mod config;
mod endpoints;
pub mod error;
pub mod storage;

use std::sync::Arc;

use ed25519_dalek::{SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey};
use title_types::*;

use config::GatewayState;
use storage::S3TempStorage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // 環境変数の読み込み
    let tee_endpoint =
        std::env::var("TEE_ENDPOINT").unwrap_or_else(|_| "http://localhost:4000".to_string());

    // Gateway認証用Ed25519キーペア
    let signing_key = if let Ok(key_hex) = std::env::var("GATEWAY_SIGNING_KEY") {
        let key_bytes = hex::decode(&key_hex)?;
        let key_arr: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("GATEWAY_SIGNING_KEYは32バイトの16進数である必要があります"))?;
        Ed25519SigningKey::from_bytes(&key_arr)
    } else {
        tracing::warn!("GATEWAY_SIGNING_KEYが未設定です。ランダムキーを生成します（開発環境用）");
        Ed25519SigningKey::generate(&mut rand::rngs::OsRng)
    };
    let verifying_key = Ed25519VerifyingKey::from(&signing_key);
    {
        use base58::ToBase58;
        tracing::info!(
            gateway_pubkey = %verifying_key.to_bytes().to_base58(),
            "Gateway署名用公開鍵"
        );
    }

    // Temporary Storage（S3互換）
    let temp_storage = S3TempStorage::from_env()?;

    // Solana RPC（sign-and-mint用、オプション）
    let solana_rpc_url = std::env::var("SOLANA_RPC_URL").ok();
    let solana_keypair = std::env::var("GATEWAY_SOLANA_KEYPAIR").ok().map(|s| {
        solana_sdk::signer::keypair::Keypair::from_base58_string(&s)
    });

    // サポートするExtensionリスト
    let supported_extensions = vec![
        "core-c2pa".to_string(),
        "phash-v1".to_string(),
        "hardware-google".to_string(),
        "c2pa-training-v1".to_string(),
        "c2pa-license-v1".to_string(),
    ];

    let state = Arc::new(GatewayState {
        tee_endpoint,
        http_client: reqwest::Client::new(),
        signing_key,
        verifying_key,
        temp_storage: Box::new(temp_storage),
        solana_rpc_url,
        solana_keypair,
        supported_extensions,
        node_limits: NodeLimits {
            max_single_content_bytes: 2 * 1024 * 1024 * 1024, // 2GB
            max_concurrent_bytes: 8 * 1024 * 1024 * 1024,     // 8GB
        },
        default_resource_limits: ResourceLimits {
            max_single_content_bytes: Some(2 * 1024 * 1024 * 1024),
            max_concurrent_bytes: Some(8 * 1024 * 1024 * 1024),
            min_upload_speed_bytes: Some(1024 * 1024),
            base_processing_time_sec: Some(30),
            max_global_timeout_sec: Some(3600),
            chunk_read_timeout_sec: Some(30),
            c2pa_max_graph_size: Some(10000),
        },
        max_upload_size: 2 * 1024 * 1024 * 1024, // 2GB
        presign_expiry_secs: 3600,
    });

    let app = axum::Router::new()
        .route("/upload-url", axum::routing::post(endpoints::handle_upload_url))
        .route("/verify", axum::routing::post(endpoints::handle_verify))
        .route("/sign", axum::routing::post(endpoints::handle_sign))
        .route("/sign-and-mint", axum::routing::post(endpoints::handle_sign_and_mint))
        .route(
            "/.well-known/title-node-info",
            axum::routing::get(endpoints::handle_node_info),
        )
        .with_state(state);

    let addr = "0.0.0.0:3000";
    tracing::info!("Gatewayを {} で起動します", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use auth::{b64, build_gateway_auth_wrapper};
    use base58::ToBase58;
    use config::GatewayState;
    use endpoints::*;
    use storage::{PresignedUrls, TempStorage};

    use axum::extract::State;
    use axum::Json;
    use base64::Engine;

    /// テスト用のモックTempStorage。
    /// S3への接続なしで署名付きURLのダミーを返す。
    struct MockTempStorage;

    #[async_trait::async_trait]
    impl TempStorage for MockTempStorage {
        async fn generate_presigned_urls(
            &self,
            object_key: &str,
            _expiry_secs: u32,
        ) -> Result<PresignedUrls, error::GatewayError> {
            Ok(PresignedUrls {
                upload_url: format!("http://mock-storage/upload/{object_key}?sig=test"),
                download_url: format!("http://mock-storage/download/{object_key}?sig=test"),
            })
        }
    }

    /// テスト用GatewayStateを構築するヘルパー
    fn test_state(tee_endpoint: &str) -> Arc<GatewayState> {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = Ed25519VerifyingKey::from(&signing_key);

        Arc::new(GatewayState {
            tee_endpoint: tee_endpoint.to_string(),
            http_client: reqwest::Client::new(),
            signing_key,
            verifying_key,
            temp_storage: Box::new(MockTempStorage),
            solana_rpc_url: None,
            solana_keypair: None,
            supported_extensions: vec![],
            node_limits: NodeLimits {
                max_single_content_bytes: 1024,
                max_concurrent_bytes: 4096,
            },
            default_resource_limits: ResourceLimits {
                max_single_content_bytes: Some(1024),
                max_concurrent_bytes: None,
                min_upload_speed_bytes: None,
                base_processing_time_sec: None,
                max_global_timeout_sec: None,
                chunk_read_timeout_sec: None,
                c2pa_max_graph_size: None,
            },
            max_upload_size: 1024,
            presign_expiry_secs: 3600,
        })
    }

    /// Gateway認証ラッパーの署名が正しく構築・検証できることを確認
    #[test]
    fn test_gateway_auth_roundtrip() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = Ed25519VerifyingKey::from(&signing_key);

        let body = serde_json::json!({
            "download_url": "http://example.com/payload",
            "processor_ids": ["core-c2pa"]
        });

        let resource_limits = Some(ResourceLimits {
            max_single_content_bytes: Some(1024),
            max_concurrent_bytes: None,
            min_upload_speed_bytes: None,
            base_processing_time_sec: None,
            max_global_timeout_sec: None,
            chunk_read_timeout_sec: None,
            c2pa_max_graph_size: None,
        });

        let wrapper = build_gateway_auth_wrapper(
            &signing_key,
            "POST",
            "/verify",
            body.clone(),
            resource_limits.clone(),
        )
        .unwrap();

        assert_eq!(wrapper.method, "POST");
        assert_eq!(wrapper.path, "/verify");
        assert_eq!(wrapper.body, body);

        // 署名を検証
        let sign_target = GatewayAuthSignTarget {
            method: wrapper.method.clone(),
            path: wrapper.path.clone(),
            body: wrapper.body.clone(),
            resource_limits: wrapper.resource_limits.clone(),
        };
        let sign_bytes = serde_json::to_vec(&sign_target).unwrap();

        let sig_bytes = b64().decode(&wrapper.gateway_signature).unwrap();
        let sig_arr: [u8; 64] = sig_bytes.try_into().unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

        assert!(
            title_crypto::ed25519_verify(&verifying_key, &sign_bytes, &signature).is_ok(),
            "Gateway署名の検証に失敗"
        );
    }

    /// 不正な署名がGateway認証で拒否されることを確認
    #[test]
    fn test_gateway_auth_invalid_signature() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let other_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let other_verifying_key = Ed25519VerifyingKey::from(&other_key);

        let body = serde_json::json!({"test": "data"});

        let wrapper =
            build_gateway_auth_wrapper(&signing_key, "POST", "/verify", body, None).unwrap();

        // 別の公開鍵で検証 → 失敗すべき
        let sign_target = GatewayAuthSignTarget {
            method: wrapper.method.clone(),
            path: wrapper.path.clone(),
            body: wrapper.body.clone(),
            resource_limits: wrapper.resource_limits.clone(),
        };
        let sign_bytes = serde_json::to_vec(&sign_target).unwrap();

        let sig_bytes = b64().decode(&wrapper.gateway_signature).unwrap();
        let sig_arr: [u8; 64] = sig_bytes.try_into().unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

        assert!(
            title_crypto::ed25519_verify(&other_verifying_key, &sign_bytes, &signature).is_err(),
            "異なる公開鍵での検証が成功してしまった"
        );
    }

    /// /.well-known/title-node-info が正しいNodeInfoを返すことを確認
    #[tokio::test]
    async fn test_node_info() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = Ed25519VerifyingKey::from(&signing_key);
        let expected_pubkey = verifying_key.to_bytes().to_base58();

        let state = Arc::new(GatewayState {
            tee_endpoint: "http://localhost:4000".to_string(),
            http_client: reqwest::Client::new(),
            signing_key,
            verifying_key,
            temp_storage: Box::new(MockTempStorage),
            solana_rpc_url: None,
            solana_keypair: None,
            supported_extensions: vec!["core-c2pa".to_string(), "phash-v1".to_string()],
            node_limits: NodeLimits {
                max_single_content_bytes: 1024,
                max_concurrent_bytes: 4096,
            },
            default_resource_limits: ResourceLimits {
                max_single_content_bytes: Some(1024),
                max_concurrent_bytes: None,
                min_upload_speed_bytes: None,
                base_processing_time_sec: None,
                max_global_timeout_sec: None,
                chunk_read_timeout_sec: None,
                c2pa_max_graph_size: None,
            },
            max_upload_size: 1024,
            presign_expiry_secs: 3600,
        });

        let result = handle_node_info(State(state)).await;
        let node_info = result.0;

        assert_eq!(node_info.signing_pubkey, expected_pubkey);
        assert_eq!(node_info.supported_extensions, vec!["core-c2pa", "phash-v1"]);
        assert_eq!(node_info.limits.max_single_content_bytes, 1024);
        assert_eq!(node_info.limits.max_concurrent_bytes, 4096);
    }

    /// /upload-urlでサイズ上限チェックが機能することを確認
    #[tokio::test]
    async fn test_upload_url_size_limit() {
        let state = test_state("http://localhost:4000");

        // サイズ上限を超えるリクエスト → BadRequest
        let result = handle_upload_url(
            State(state.clone()),
            Json(UploadUrlRequest {
                content_size: 2048,
                content_type: "image/jpeg".to_string(),
            }),
        )
        .await;
        assert!(result.is_err());

        // サイズ0のリクエスト → BadRequest
        let result = handle_upload_url(
            State(state.clone()),
            Json(UploadUrlRequest {
                content_size: 0,
                content_type: "image/jpeg".to_string(),
            }),
        )
        .await;
        assert!(result.is_err());

        // 正常なリクエスト → presigned URLが返却される
        let result = handle_upload_url(
            State(state),
            Json(UploadUrlRequest {
                content_size: 512,
                content_type: "image/jpeg".to_string(),
            }),
        )
        .await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert!(!response.upload_url.is_empty());
        assert!(!response.download_url.is_empty());
        assert!(response.expires_at > 0);
    }

    /// モックTEEサーバーを起動し、/verify中継が正しく動作することを確認
    #[tokio::test]
    async fn test_verify_relay() {
        // モックTEEサーバーを起動
        let mock_tee = axum::Router::new().route(
            "/verify",
            axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
                // GatewayAuthWrapper形式で受信していることを確認
                assert!(body.get("gateway_signature").is_some());
                assert!(body.get("body").is_some());
                assert_eq!(body.get("method").unwrap().as_str().unwrap(), "POST");
                assert_eq!(body.get("path").unwrap().as_str().unwrap(), "/verify");

                // ダミーのEncryptedResponseを返却
                Json(serde_json::json!({
                    "nonce": "dGVzdG5vbmNlMTIz",
                    "ciphertext": "ZW5jcnlwdGVk"
                }))
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, mock_tee).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let state = test_state(&format!("http://127.0.0.1:{port}"));

        let result = handle_verify(
            State(state),
            Json(VerifyRequest {
                download_url: "http://example.com/payload".to_string(),
                processor_ids: vec!["core-c2pa".to_string()],
            }),
        )
        .await;

        assert!(result.is_ok(), "handle_verify failed: {:?}", result.err());
        let response = result.unwrap().0;
        assert!(response.get("nonce").is_some());
        assert!(response.get("ciphertext").is_some());
    }

    /// モックTEEサーバーを起動し、/sign中継が正しく動作することを確認
    #[tokio::test]
    async fn test_sign_relay() {
        let mock_tee = axum::Router::new().route(
            "/sign",
            axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
                assert!(body.get("gateway_signature").is_some());
                Json(serde_json::json!({
                    "partial_txs": ["dGVzdHR4"]
                }))
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, mock_tee).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let state = test_state(&format!("http://127.0.0.1:{port}"));

        let result = handle_sign(
            State(state),
            Json(SignRequest {
                recent_blockhash: "11111111111111111111111111111111".to_string(),
                requests: vec![SignRequestItem {
                    signed_json_uri: "ar://test".to_string(),
                }],
            }),
        )
        .await;

        assert!(result.is_ok(), "handle_sign failed: {:?}", result.err());
        let response = result.unwrap().0;
        assert_eq!(response.partial_txs.len(), 1);
    }
}
