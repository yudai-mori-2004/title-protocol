// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol ローカル Temporary Storage サーバー
//!
//! 仕様書 §6.3
//!
//! 素朴なHTTP PUT/GETファイルサーバー。
//! ローカル開発専用のため、認証は不要。
//!
//! ## エンドポイント
//! - `PUT /objects/:key` — バイナリボディをファイルに保存
//! - `GET /objects/:key` — ファイルを返却
//! - `GET /health` — ヘルスチェック
//!
//! ## 環境変数
//! - `STORAGE_DIR` — 保存先ディレクトリ（デフォルト: `/tmp/title-uploads/`）
//! - `STORAGE_PORT` — リッスンポート（デフォルト: `3001`）

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

struct AppState {
    storage_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let storage_dir = PathBuf::from(
        std::env::var("STORAGE_DIR").unwrap_or_else(|_| "/tmp/title-uploads".to_string()),
    );
    let port = std::env::var("STORAGE_PORT").unwrap_or_else(|_| "3001".to_string());

    tokio::fs::create_dir_all(&storage_dir).await?;
    tracing::info!(dir = %storage_dir.display(), "ストレージディレクトリ");

    let state = Arc::new(AppState { storage_dir });

    let app = axum::Router::new()
        .route("/objects/{*key}", axum::routing::put(handle_put))
        .route("/objects/{*key}", axum::routing::get(handle_get))
        .route("/health", axum::routing::get(handle_health))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("TempStorageサーバーを {} で起動します", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// PUT /objects/:key — バイナリボディをファイルに保存
/// 仕様書 §6.3
async fn handle_put(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let safe_key = sanitize_key(&key);
    let path = state.storage_dir.join(&safe_key);

    // サブディレクトリが必要な場合は作成
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            tracing::error!(error = %e, "ディレクトリ作成失敗");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }

    match tokio::fs::write(&path, &body).await {
        Ok(()) => {
            tracing::info!(key = %safe_key, size = body.len(), "保存完了");
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!(error = %e, key = %safe_key, "ファイル書き込み失敗");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// GET /objects/:key — ファイルを返却（Content-Length付き）
/// 仕様書 §6.3
async fn handle_get(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let safe_key = sanitize_key(&key);
    let path = state.storage_dir.join(&safe_key);

    match tokio::fs::read(&path).await {
        Ok(data) => {
            tracing::info!(key = %safe_key, size = data.len(), "読み取り完了");
            (
                StatusCode::OK,
                [("content-length", data.len().to_string())],
                data,
            )
                .into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, "オブジェクトが見つかりません").into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, key = %safe_key, "ファイル読み取り失敗");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /health — ヘルスチェック
async fn handle_health() -> &'static str {
    "ok"
}

/// キーのサニタイズ: パストラバーサルを防止
fn sanitize_key(key: &str) -> String {
    key.replace("..", "_").replace('/', "_")
}
