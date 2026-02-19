//! # /sign エンドポイント
//!
//! 仕様書 §6.4 /signフェーズの内部処理
//!
//! ## 処理フロー
//! 1. signed_json_uriからJSONをフェッチ
//! 2. JSON内のtee_signatureを自身の公開鍵で検証
//! 3. payload.creator_walletを宛先としてcNFT発行トランザクションを構築
//! 4. TEEの秘密鍵で部分署名

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::{AppState, TeeState};

/// /sign エンドポイントハンドラ。
/// 仕様書 §6.4
pub async fn handle_sign(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    // active状態チェック
    {
        let current = state.state.read().await;
        if *current != TeeState::Active {
            return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    // TODO: 1. Gateway署名の検証
    // TODO: 2. signed_json_uriからJSONをfetch（サイズ制限付き）
    // TODO: 3. tee_signatureを自身の公開鍵で検証
    // TODO: 4. cNFT発行トランザクションの構築
    // TODO: 5. TEE秘密鍵での部分署名

    let _ = body;
    todo!("/sign エンドポイントの実装")
}
