// SPDX-License-Identifier: Apache-2.0

//! # /set-alt エンドポイント
//!
//! Address Lookup Table の設定。
//! setup.sh から /create-tree の後に呼ばれ、ALT アドレスと
//! ALT に登録されたアカウント一覧を TEE に設定する。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::config::TeeAppState;
use crate::error::TeeError;

/// /set-alt リクエスト。
#[derive(serde::Deserialize)]
pub struct SetAltRequest {
    /// ALT アカウントのBase58アドレス。
    pub alt_address: String,
    /// ALT に登録されたアカウント一覧（Base58、カンマ区切り）。
    pub addresses: Vec<String>,
}

/// /set-alt レスポンス。
#[derive(serde::Serialize)]
pub struct SetAltResponse {
    pub alt_address: String,
    pub num_addresses: usize,
}

/// /set-alt エンドポイントハンドラ。
pub async fn handle_set_alt(
    State(state): State<Arc<TeeAppState>>,
    Json(request): Json<SetAltRequest>,
) -> Result<Json<SetAltResponse>, TeeError> {
    let alt_pubkey = Pubkey::from_str(&request.alt_address)
        .map_err(|e| TeeError::BadRequest(format!("alt_addressのパースに失敗: {e}")))?;

    let mut addresses = Vec::with_capacity(request.addresses.len());
    for addr_str in &request.addresses {
        let pubkey = Pubkey::from_str(addr_str)
            .map_err(|e| TeeError::BadRequest(format!("addressのパースに失敗 ({addr_str}): {e}")))?;
        addresses.push(pubkey);
    }

    let num_addresses = addresses.len();

    {
        let mut alt_addr = state.alt_address.write().await;
        *alt_addr = Some(alt_pubkey);
    }
    {
        let mut alt_addrs = state.alt_addresses.write().await;
        *alt_addrs = addresses;
    }

    tracing::info!(alt_address = %alt_pubkey, num_addresses, "ALT設定完了");

    Ok(Json(SetAltResponse {
        alt_address: alt_pubkey.to_string(),
        num_addresses,
    }))
}
