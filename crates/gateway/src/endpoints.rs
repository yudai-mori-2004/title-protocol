// SPDX-License-Identifier: Apache-2.0

//! # Gateway Endpoint Handlers
//!
//! Spec §2.5, §5.3
//!
//! Six endpoints as defined in the spec:
//! - GET  /keys            -- TEE encryption public keys
//! - GET  /processors      -- Supported processor list
//! - POST /process         -- Attribute extraction relay
//! - GET  /health          -- TEE availability
//! - GET  /solana-keys     -- Solana Extension public keys
//! - POST /extension/solana -- Solana Extension relay

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use title_core::ProcessRequest;

use crate::error::GatewayError;
use crate::state::GatewayState;
use crate::tee_client::{ProcessOutcome, TeeClientError};
use crate::{
    CreateTreeRequest, CreateTreeResponse, HealthResponse, KeysResponse, ProcessorsResponse,
    SolanaExtensionRequest, SolanaExtensionResponse, SolanaKeysResponse,
};

// ---------------------------------------------------------------------------
// Error conversion
// ---------------------------------------------------------------------------

fn tee_err(e: TeeClientError) -> GatewayError {
    match e {
        TeeClientError::Unreachable(msg) => GatewayError::TeeUnavailable(msg),
        TeeClientError::HttpError { status, body } => {
            tracing::warn!(status, body = %body, "TEE returned HTTP error");
            // body は client にも透過する。上流 TEE が返した詳細メッセージ
            // (例: "Base58 decode failed: ...") を Gateway 側で固定文字列に
            // 潰すと debug 不能になるため、IntoResponse 側で `detail` フィールドに
            // 包んで返す。
            match status {
                503 => GatewayError::TeeUnavailable(format!("TEE upstream returned HTTP {status}")),
                429 => GatewayError::RateLimited,
                400..=499 => GatewayError::TeeRejected { status, body },
                500..=599 => GatewayError::TeeUpstreamError { status, body },
                _ => GatewayError::TeeError(format!("TEE upstream returned HTTP {status}")),
            }
        }
        TeeClientError::ParseError(msg) => GatewayError::TeeError(msg),
    }
}

// ---------------------------------------------------------------------------
// GET /keys (§2.5)
// ---------------------------------------------------------------------------

/// GET /keys -- Return cached TEE encryption public keys.
/// Spec §2.5
///
/// Keys are cached by the Gateway and refreshed when TEE restarts (§5.3).
pub async fn handle_keys(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<KeysResponse>, GatewayError> {
    let cache = state.tee_cache.read().await;
    cache
        .keys
        .clone()
        .map(Json)
        .ok_or_else(|| GatewayError::TeeUnavailable("TEE keys not yet available".into()))
}

// ---------------------------------------------------------------------------
// GET /processors (§2.5)
// ---------------------------------------------------------------------------

/// GET /processors -- Return cached processor list.
/// Spec §2.5
pub async fn handle_processors(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ProcessorsResponse>, GatewayError> {
    let cache = state.tee_cache.read().await;
    cache
        .processors
        .clone()
        .map(Json)
        .ok_or_else(|| GatewayError::TeeUnavailable("TEE processor list not yet available".into()))
}

// ---------------------------------------------------------------------------
// POST /process (§2.5)
// ---------------------------------------------------------------------------

/// POST /process — Relay attribute extraction request to TEE.
/// Spec §2.5, §5.3.
///
/// Plaintext requests get a JSON `ProcessResponse`; encrypted requests
/// (spec §2.4) get `application/octet-stream` with the sealed bytes
/// forwarded verbatim.
pub async fn handle_process(
    State(state): State<Arc<GatewayState>>,
    Json(request): Json<ProcessRequest>,
) -> Result<Response, GatewayError> {
    if !state.is_tee_available() {
        return Err(GatewayError::TeeUnavailable("TEE is not available".into()));
    }

    match state.tee_client.process(&request).await.map_err(tee_err)? {
        ProcessOutcome::Plaintext(body) => Ok(Json(body).into_response()),
        ProcessOutcome::Encrypted(bytes) => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response()),
    }
}

// ---------------------------------------------------------------------------
// GET /health (§2.5)
// ---------------------------------------------------------------------------

/// GET /health -- Return TEE health status.
/// Spec §2.5
///
/// Always responds (even without auth). Returns cached TEE type if available,
/// or probes TEE directly.
pub async fn handle_health(State(state): State<Arc<GatewayState>>) -> Json<HealthResponse> {
    // /health は LB の readiness probe や Gateway 自身の health checker から
    // 高頻度で叩かれる。read ガードは最短スコープで落として `refresh_tee_info`
    // の write を待たせない。
    let tee_type = {
        let cache = state.tee_cache.read().await;
        cache.tee_type.clone()
    };
    let status = if state.is_tee_available() {
        "ok".to_string()
    } else {
        "unavailable".to_string()
    };
    Json(HealthResponse { status, tee_type })
}

// ---------------------------------------------------------------------------
// GET /solana-keys (§2.5)
// ---------------------------------------------------------------------------

/// GET /solana-keys -- Return Solana Extension public keys.
/// Spec §2.5
///
/// Returns 404 if Solana Extension is not enabled.
pub async fn handle_solana_keys(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<SolanaKeysResponse>, GatewayError> {
    let cache = state.tee_cache.read().await;
    cache
        .solana_keys
        .clone()
        .map(Json)
        .ok_or_else(|| GatewayError::NotFound("Solana Extension not enabled".into()))
}

// ---------------------------------------------------------------------------
// POST /extension/solana (§2.5, §6.2)
// ---------------------------------------------------------------------------

/// POST /solana/create-tree -- Relay Merkle tree creation to TEE.
/// Spec §6.2
pub async fn handle_create_tree(
    State(state): State<Arc<GatewayState>>,
    Json(request): Json<CreateTreeRequest>,
) -> Result<Json<CreateTreeResponse>, GatewayError> {
    if !state.is_tee_available() {
        return Err(GatewayError::TeeUnavailable("TEE is not available".into()));
    }

    {
        let cache = state.tee_cache.read().await;
        if cache.solana_keys.is_none() {
            return Err(GatewayError::NotFound(
                "Solana Extension not enabled".into(),
            ));
        }
    }

    state
        .tee_client
        .create_tree(&request)
        .await
        .map(Json)
        .map_err(tee_err)
}

/// POST /extension/solana -- Relay Solana Extension request to TEE.
/// Spec §2.5, §6.2
pub async fn handle_solana_extension(
    State(state): State<Arc<GatewayState>>,
    Json(request): Json<SolanaExtensionRequest>,
) -> Result<Json<SolanaExtensionResponse>, GatewayError> {
    // Order matters: a downed TEE returns 503 (transient), which beats the
    // 404 we'd otherwise return for the extension cache being empty. Once
    // the TEE is back up the cache is rebuilt and the 404 path becomes
    // a real "Solana Extension not enabled" answer.
    if !state.is_tee_available() {
        return Err(GatewayError::TeeUnavailable("TEE is not available".into()));
    }

    // tokio::sync::RwLock の read ガードを下の `.await` 越しに持ち越すと
    // `refresh_tee_info` の writer がこのリクエスト完了まで待たされる。
    // 明示スコープで guard を確実に drop してから upstream に呼びに行く。
    {
        let cache = state.tee_cache.read().await;
        if cache.solana_keys.is_none() {
            return Err(GatewayError::NotFound(
                "Solana Extension not enabled".into(),
            ));
        }
    }

    state
        .tee_client
        .solana_extension(&request)
        .await
        .map(Json)
        .map_err(tee_err)
}
