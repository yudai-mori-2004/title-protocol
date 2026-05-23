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
use axum::Json;

use title_core::{ProcessRequest, ProcessResponse};

use crate::error::GatewayError;
use crate::state::GatewayState;
use crate::tee_client::TeeClientError;
use crate::{
    HealthResponse, KeysResponse, ProcessorsResponse, SolanaExtensionRequest,
    SolanaExtensionResponse, SolanaKeysResponse,
};

// ---------------------------------------------------------------------------
// Error conversion
// ---------------------------------------------------------------------------

fn tee_err(e: TeeClientError) -> GatewayError {
    match e {
        TeeClientError::Unreachable(msg) => GatewayError::TeeUnavailable(msg),
        TeeClientError::HttpError { status, body } => {
            GatewayError::TeeError(format!("HTTP {status}: {body}"))
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

/// POST /process -- Relay attribute extraction request to TEE.
/// Spec §2.5, §5.3
///
/// The Gateway authenticates the client, then forwards the ProcessRequest
/// to the TEE and returns the ProcessResponse. If the TEE is unavailable,
/// returns 503.
pub async fn handle_process(
    State(state): State<Arc<GatewayState>>,
    Json(request): Json<ProcessRequest>,
) -> Result<Json<ProcessResponse>, GatewayError> {
    if !state.is_tee_available() {
        return Err(GatewayError::TeeUnavailable(
            "TEE is not available".into(),
        ));
    }

    state
        .tee_client
        .process(&request)
        .await
        .map(Json)
        .map_err(tee_err)
}

// ---------------------------------------------------------------------------
// GET /health (§2.5)
// ---------------------------------------------------------------------------

/// GET /health -- Return TEE health status.
/// Spec §2.5
///
/// Always responds (even without auth). Returns cached TEE type if available,
/// or probes TEE directly.
pub async fn handle_health(
    State(state): State<Arc<GatewayState>>,
) -> Json<HealthResponse> {
    let cache = state.tee_cache.read().await;
    let tee_type = cache.tee_type.clone();
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

/// POST /extension/solana -- Relay Solana Extension request to TEE.
/// Spec §2.5, §6.2
pub async fn handle_solana_extension(
    State(state): State<Arc<GatewayState>>,
    Json(request): Json<SolanaExtensionRequest>,
) -> Result<Json<SolanaExtensionResponse>, GatewayError> {
    if !state.is_tee_available() {
        return Err(GatewayError::TeeUnavailable(
            "TEE is not available".into(),
        ));
    }

    // Check if Solana Extension is enabled
    {
        let cache = state.tee_cache.read().await;
        if cache.solana_keys.is_none() {
            return Err(GatewayError::NotFound("Solana Extension not enabled".into()));
        }
    }

    state
        .tee_client
        .solana_extension(&request)
        .await
        .map(Json)
        .map_err(tee_err)
}
