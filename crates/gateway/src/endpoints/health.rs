// SPDX-License-Identifier: Apache-2.0

//! # GET /health
//!
//! ノードのステータスとcapabilitiesを返す。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::config::GatewayState;

/// GET /health — ノードのステータスとcapabilitiesを返す。
///
/// `capabilities.store_signed_json` が `true` の場合、
/// `/sign-and-mint` で `signed_json` 本体を受け取り保存を代行できる。
pub async fn handle_health(
    State(state): State<Arc<GatewayState>>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "capabilities": {
            "store_signed_json": state.signed_json_storage.is_some(),
        }
    }))
}
