//! # POST /sign
//!
//! 仕様書 §6.2
//!
//! TEEへのリクエスト中継。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use title_types::*;

use crate::auth::relay_to_tee;
use crate::config::GatewayState;
use crate::error::GatewayError;

/// POST /sign — TEEへのリクエスト中継。
/// 仕様書 §6.2
///
/// クライアントのSignRequestをGateway認証で包み、TEEに中継する。
/// TEEからの部分署名済みトランザクションをクライアントに返す。
pub async fn handle_sign(
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
