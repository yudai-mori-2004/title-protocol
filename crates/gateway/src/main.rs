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

/// Gatewayの共有状態。
pub struct GatewayState {
    /// TEEのエンドポイントURL
    pub tee_endpoint: String,
    /// HTTPクライアント
    pub http_client: reqwest::Client,
    // TODO: Gateway署名用Ed25519キーペア
    // TODO: Temporary Storage (S3/MinIO) クライアント
    // TODO: レート制限設定
}

/// POST /upload-url — 署名付きURL発行。
/// 仕様書 §6.2
async fn handle_upload_url(
    axum::extract::State(_state): axum::extract::State<Arc<GatewayState>>,
    axum::Json(_body): axum::Json<title_types::UploadUrlRequest>,
) -> Result<axum::Json<title_types::UploadUrlResponse>, axum::http::StatusCode> {
    // TODO: Temporary Storageへの署名付きURL発行
    // TODO: content-length-range条件の設定（EDoS攻撃対策）
    todo!("/upload-url エンドポイントの実装")
}

/// POST /verify — TEEへのリクエスト中継 + Gateway認証署名付与。
/// 仕様書 §6.2
async fn handle_verify(
    axum::extract::State(_state): axum::extract::State<Arc<GatewayState>>,
    axum::Json(_body): axum::Json<title_types::VerifyRequest>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    // TODO: Gateway認証ラッパーの構築
    //   - method, path, body, resource_limits を署名
    // TODO: TEEへのリクエスト中継
    // TODO: レスポンスの返却（暗号化されたまま）
    todo!("/verify エンドポイントの実装")
}

/// POST /sign — TEEへのリクエスト中継。
/// 仕様書 §6.2
async fn handle_sign(
    axum::extract::State(_state): axum::extract::State<Arc<GatewayState>>,
    axum::Json(_body): axum::Json<title_types::SignRequest>,
) -> Result<axum::Json<title_types::SignResponse>, axum::http::StatusCode> {
    // TODO: Gateway認証ラッパーの構築
    // TODO: TEEへのリクエスト中継
    todo!("/sign エンドポイントの実装")
}

/// POST /sign-and-mint — sign + ブロードキャスト代行。
/// 仕様書 §6.2
async fn handle_sign_and_mint(
    axum::extract::State(_state): axum::extract::State<Arc<GatewayState>>,
    axum::Json(_body): axum::Json<title_types::SignRequest>,
) -> Result<axum::Json<title_types::SignAndMintResponse>, axum::http::StatusCode> {
    // TODO: /sign と同様の処理
    // TODO: 最終署名 + Solanaへのブロードキャスト代行
    todo!("/sign-and-mint エンドポイントの実装")
}

/// GET /.well-known/title-node-info — ノード情報公開。
/// 仕様書 §6.2
async fn handle_node_info(
    axum::extract::State(_state): axum::extract::State<Arc<GatewayState>>,
) -> axum::Json<title_types::NodeInfo> {
    // TODO: ノード情報の返却
    todo!("/.well-known/title-node-info エンドポイントの実装")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let tee_endpoint =
        std::env::var("TEE_ENDPOINT").unwrap_or_else(|_| "http://localhost:4000".to_string());

    let state = Arc::new(GatewayState {
        tee_endpoint,
        http_client: reqwest::Client::new(),
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
