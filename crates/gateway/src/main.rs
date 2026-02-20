//! # Title Protocol Gateway
//!
//! 仕様書セクション6.2で定義されるGateway。
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

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey};
use title_types::*;

// ---------------------------------------------------------------------------
// ユーティリティ
// ---------------------------------------------------------------------------

/// Base64エンジン（Standard）
fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

// ---------------------------------------------------------------------------
// エラー型
// ---------------------------------------------------------------------------

/// Gatewayエラー型。
/// 仕様書 §6.2
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// TEEへのリクエスト中継に失敗
    #[error("TEEへのリクエスト中継に失敗: {0}")]
    TeeRelay(String),
    /// ストレージ操作に失敗
    #[error("ストレージ操作に失敗: {0}")]
    Storage(String),
    /// Solana RPC エラー
    #[error("Solana RPC エラー: {0}")]
    Solana(String),
    /// 内部エラー
    #[error("内部エラー: {0}")]
    Internal(String),
    /// 不正なリクエスト
    #[error("不正なリクエスト: {0}")]
    BadRequest(String),
}

impl axum::response::IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            GatewayError::TeeRelay(_) => StatusCode::BAD_GATEWAY,
            GatewayError::Storage(_) | GatewayError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            GatewayError::Solana(_) => StatusCode::BAD_GATEWAY,
            GatewayError::BadRequest(_) => StatusCode::BAD_REQUEST,
        };
        (status, self.to_string()).into_response()
    }
}

// ---------------------------------------------------------------------------
// Temporary Storage トレイト (仕様書 §6.3)
// ---------------------------------------------------------------------------

/// Temporary Storageの署名付きURL生成結果。
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

// ---------------------------------------------------------------------------
// S3互換 Temporary Storage 実装
// ---------------------------------------------------------------------------

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
        let region = s3::Region::Custom {
            region: "us-east-1".to_string(),
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
    pub fn from_env() -> anyhow::Result<Self> {
        let endpoint = std::env::var("MINIO_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:9000".to_string());
        let access_key =
            std::env::var("MINIO_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let secret_key =
            std::env::var("MINIO_SECRET_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let bucket_name =
            std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "title-uploads".to_string());

        let bucket_internal =
            Self::init_bucket(&endpoint, &access_key, &secret_key, &bucket_name)?;

        let bucket_public = std::env::var("MINIO_PUBLIC_ENDPOINT")
            .ok()
            .map(|public_ep| {
                tracing::info!(
                    minio_public_endpoint = %public_ep,
                    "クライアント向けMinIOエンドポイントを設定"
                );
                Self::init_bucket(&public_ep, &access_key, &secret_key, &bucket_name)
            })
            .transpose()?;

        Ok(Self::new(bucket_internal, bucket_public))
    }
}

#[async_trait::async_trait]
impl TempStorage for S3TempStorage {
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
// 共有状態
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Gateway認証ヘルパー
// ---------------------------------------------------------------------------

/// Gateway認証ラッパーを構築する。
/// 仕様書 §6.2: リクエスト内容 + resource_limits を含む構造体を構築し、Gateway秘密鍵で署名する。
fn build_gateway_auth_wrapper(
    signing_key: &Ed25519SigningKey,
    method: &str,
    path: &str,
    body: serde_json::Value,
    resource_limits: Option<ResourceLimits>,
) -> Result<GatewayAuthWrapper, GatewayError> {
    let sign_target = GatewayAuthSignTarget {
        method: method.to_string(),
        path: path.to_string(),
        body: body.clone(),
        resource_limits: resource_limits.clone(),
    };

    let sign_bytes = serde_json::to_vec(&sign_target)
        .map_err(|e| GatewayError::Internal(format!("署名対象のシリアライズに失敗: {e}")))?;

    let signature = signing_key.sign(&sign_bytes);
    let signature_b64 = b64().encode(signature.to_bytes());

    Ok(GatewayAuthWrapper {
        method: method.to_string(),
        path: path.to_string(),
        body,
        resource_limits,
        gateway_signature: signature_b64,
    })
}

/// TEEにリクエストを中継する。
/// 仕様書 §6.2: Gateway認証署名を付与してTEEにリクエストを転送する。
async fn relay_to_tee(
    state: &GatewayState,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, GatewayError> {
    let wrapper = build_gateway_auth_wrapper(
        &state.signing_key,
        "POST",
        path,
        body,
        Some(state.default_resource_limits.clone()),
    )?;

    let url = format!("{}{}", state.tee_endpoint, path);
    let response = state
        .http_client
        .post(&url)
        .json(&wrapper)
        .send()
        .await
        .map_err(|e| GatewayError::TeeRelay(format!("HTTP送信失敗: {e}")))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| GatewayError::TeeRelay(format!("レスポンス読み取り失敗: {e}")))?;

    if !status.is_success() {
        return Err(GatewayError::TeeRelay(format!(
            "TEEがエラーを返しました: HTTP {} - {}",
            status, response_body
        )));
    }

    serde_json::from_str(&response_body)
        .map_err(|e| GatewayError::TeeRelay(format!("レスポンスのパースに失敗: {e}")))
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

/// POST /upload-url — 署名付きURL発行。
/// 仕様書 §6.2
///
/// Temporary Storageへのアップロード用署名付きURLを発行する。
/// content-length-range条件によるEDoS攻撃対策を含む。
async fn handle_upload_url(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<UploadUrlRequest>,
) -> Result<Json<UploadUrlResponse>, GatewayError> {
    // EDoS対策: コンテンツサイズの上限チェック (仕様書 §6.2)
    if body.content_size > state.max_upload_size {
        return Err(GatewayError::BadRequest(format!(
            "コンテンツサイズが上限を超えています: {} bytes (上限: {} bytes)",
            body.content_size, state.max_upload_size
        )));
    }

    if body.content_size == 0 {
        return Err(GatewayError::BadRequest(
            "コンテンツサイズは1以上である必要があります".to_string(),
        ));
    }

    // ユニークなオブジェクトキーを生成
    let object_key = format!("uploads/{}", uuid::Uuid::new_v4());

    // TempStorageトレイト経由で署名付きURLを生成
    let urls = state
        .temp_storage
        .generate_presigned_urls(&object_key, state.presign_expiry_secs)
        .await?;

    // URL有効期限のUNIXタイムスタンプ
    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| GatewayError::Internal(format!("時刻取得失敗: {e}")))?
        .as_secs()
        + state.presign_expiry_secs as u64;

    Ok(Json(UploadUrlResponse {
        upload_url: urls.upload_url,
        download_url: urls.download_url,
        expires_at,
    }))
}

/// POST /verify — TEEへのリクエスト中継 + Gateway認証署名付与。
/// 仕様書 §6.2
///
/// クライアントのVerifyRequestをGateway認証で包み、TEEに中継する。
/// TEEからのレスポンス（暗号化済み）をそのままクライアントに返す。
async fn handle_verify(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<VerifyRequest>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let body_value = serde_json::to_value(&body)
        .map_err(|e| GatewayError::Internal(format!("リクエストのシリアライズに失敗: {e}")))?;

    let result = relay_to_tee(&state, "/verify", body_value).await?;
    Ok(Json(result))
}

/// POST /sign — TEEへのリクエスト中継。
/// 仕様書 §6.2
///
/// クライアントのSignRequestをGateway認証で包み、TEEに中継する。
/// TEEからの部分署名済みトランザクションをクライアントに返す。
async fn handle_sign(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<SignRequest>,
) -> Result<Json<SignResponse>, GatewayError> {
    let body_value = serde_json::to_value(&body)
        .map_err(|e| GatewayError::Internal(format!("リクエストのシリアライズに失敗: {e}")))?;

    let result = relay_to_tee(&state, "/sign", body_value).await?;

    let sign_response: SignResponse = serde_json::from_value(result)
        .map_err(|e| GatewayError::TeeRelay(format!("SignResponseのパースに失敗: {e}")))?;

    Ok(Json(sign_response))
}

/// POST /sign-and-mint — sign + ブロードキャスト代行。
/// 仕様書 §6.2
///
/// /signと同様にTEEから部分署名済みトランザクションを取得し、
/// GatewayのSolanaウォレットで最終署名を行い、Solanaにブロードキャストする。
/// クライアントはSolanaウォレットでの署名を省略でき、ガス代はGateway運営者が負担する。
async fn handle_sign_and_mint(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<SignRequest>,
) -> Result<Json<SignAndMintResponse>, GatewayError> {
    let solana_rpc_url = state
        .solana_rpc_url
        .as_ref()
        .ok_or_else(|| GatewayError::Internal("SOLANA_RPC_URLが設定されていません".to_string()))?;
    let gateway_keypair = state.solana_keypair.as_ref().ok_or_else(|| {
        GatewayError::Internal("GATEWAY_SOLANA_KEYPAIRが設定されていません".to_string())
    })?;

    // Step 1: TEEの/signに中継
    let body_value = serde_json::to_value(&body)
        .map_err(|e| GatewayError::Internal(format!("リクエストのシリアライズに失敗: {e}")))?;

    let result = relay_to_tee(&state, "/sign", body_value).await?;
    let sign_response: SignResponse = serde_json::from_value(result)
        .map_err(|e| GatewayError::TeeRelay(format!("SignResponseのパースに失敗: {e}")))?;

    // Step 2: 各partial_txにGatewayウォレットで署名+ブロードキャスト
    let mut tx_signatures = Vec::new();

    for partial_tx_b64 in &sign_response.partial_txs {
        let tx_bytes = b64().decode(partial_tx_b64).map_err(|e| {
            GatewayError::TeeRelay(format!("partial_txのBase64デコードに失敗: {e}"))
        })?;

        let mut tx: solana_sdk::transaction::Transaction =
            bincode::deserialize(&tx_bytes).map_err(|e| {
                GatewayError::TeeRelay(format!(
                    "トランザクションのデシリアライズに失敗: {e}"
                ))
            })?;

        // Gatewayウォレットで署名（未署名のスロットに署名）
        use solana_sdk::signer::Signer;
        let gateway_pubkey = gateway_keypair.pubkey();

        // Gatewayの公開鍵に対応する署名スロットを特定
        let sig_index = tx
            .message
            .account_keys
            .iter()
            .position(|k| *k == gateway_pubkey)
            .ok_or_else(|| {
                GatewayError::Internal(
                    "Gatewayの公開鍵がトランザクションの署名者に含まれていません".to_string(),
                )
            })?;

        let message_bytes = tx.message.serialize();
        let sig = gateway_keypair.sign_message(&message_bytes);
        tx.signatures[sig_index] = sig;

        // Solana RPCにブロードキャスト
        let tx_serialized = bincode::serialize(&tx)
            .map_err(|e| GatewayError::Internal(format!("トランザクションのシリアライズに失敗: {e}")))?;
        let tx_b64 = b64().encode(&tx_serialized);

        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [tx_b64, {"encoding": "base64"}]
        });

        let rpc_response = state
            .http_client
            .post(solana_rpc_url)
            .json(&rpc_request)
            .send()
            .await
            .map_err(|e| GatewayError::Solana(format!("RPC送信失敗: {e}")))?;

        let rpc_body: serde_json::Value = rpc_response
            .json()
            .await
            .map_err(|e| GatewayError::Solana(format!("RPCレスポンスのパースに失敗: {e}")))?;

        if let Some(error) = rpc_body.get("error") {
            return Err(GatewayError::Solana(format!(
                "トランザクションのブロードキャストに失敗: {error}"
            )));
        }

        let tx_sig = rpc_body
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                GatewayError::Solana("RPCレスポンスにresultがありません".to_string())
            })?;

        tx_signatures.push(tx_sig.to_string());
    }

    Ok(Json(SignAndMintResponse { tx_signatures }))
}

/// GET /.well-known/title-node-info — ノード情報公開。
/// 仕様書 §6.2
///
/// クライアント（SDK）がノードを選択するために必要なスペック情報を返却する。
async fn handle_node_info(
    State(state): State<Arc<GatewayState>>,
) -> Json<NodeInfo> {
    use base58::ToBase58;

    Json(NodeInfo {
        signing_pubkey: state.verifying_key.to_bytes().to_base58(),
        supported_extensions: state.supported_extensions.clone(),
        limits: state.node_limits.clone(),
    })
}

// ---------------------------------------------------------------------------
// エントリポイント
// ---------------------------------------------------------------------------

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
        // 開発環境用: ランダムキーを生成
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
        .route("/upload-url", axum::routing::post(handle_upload_url))
        .route("/verify", axum::routing::post(handle_verify))
        .route("/sign", axum::routing::post(handle_sign))
        .route("/sign-and-mint", axum::routing::post(handle_sign_and_mint))
        .route(
            "/.well-known/title-node-info",
            axum::routing::get(handle_node_info),
        )
        .with_state(state);

    let addr = "0.0.0.0:3000";
    tracing::info!("Gatewayを {} で起動します", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use base58::ToBase58;

    /// テスト用のモックTempStorage。
    /// S3への接続なしで署名付きURLのダミーを返す。
    struct MockTempStorage;

    #[async_trait::async_trait]
    impl TempStorage for MockTempStorage {
        async fn generate_presigned_urls(
            &self,
            object_key: &str,
            _expiry_secs: u32,
        ) -> Result<PresignedUrls, GatewayError> {
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
