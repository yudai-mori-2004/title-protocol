//! # POST /verify
//!
//! 仕様書 §6.2
//!
//! TEEへのリクエスト中継 + Gateway認証署名付与。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use title_types::*;

use crate::auth::relay_to_tee;
use crate::config::GatewayState;
use crate::error::GatewayError;

/// POST /verify — TEEへのリクエスト中継 + Gateway認証署名付与。
/// 仕様書 §6.2
///
/// クライアントのVerifyRequestをGateway認証で包み、TEEに中継する。
/// TEEからのレスポンス（暗号化済み）をそのままクライアントに返す。
pub async fn handle_verify(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<VerifyRequest>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let body_value = serde_json::to_value(&body)
        .map_err(|e| GatewayError::Internal(format!("リクエストのシリアライズに失敗: {e}")))?;

    let result = relay_to_tee(&state, "/verify", body_value).await?;
    Ok(Json(result))
}
