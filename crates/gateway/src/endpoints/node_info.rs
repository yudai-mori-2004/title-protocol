//! # GET /.well-known/title-node-info
//!
//! 仕様書 §6.2
//!
//! ノード情報公開エンドポイント。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base58::ToBase58;
use title_types::*;

use crate::config::GatewayState;

/// GET /.well-known/title-node-info — ノード情報公開。
/// 仕様書 §6.2
///
/// クライアント（SDK）がノードを選択するために必要なスペック情報を返却する。
pub async fn handle_node_info(
    State(state): State<Arc<GatewayState>>,
) -> Json<NodeInfo> {
    Json(NodeInfo {
        signing_pubkey: state.verifying_key.to_bytes().to_base58(),
        supported_extensions: state.supported_extensions.clone(),
        limits: state.node_limits.clone(),
    })
}
