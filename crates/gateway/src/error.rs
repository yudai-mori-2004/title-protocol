// SPDX-License-Identifier: Apache-2.0

//! # Gateway Error Type
//!
//! Spec §5.3
//!
//! Maps gateway errors to HTTP status codes via Axum's IntoResponse.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Gateway error. Spec §5.3.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// TEE is not reachable or not ready (503).
    #[error("TEE unavailable: {0}")]
    TeeUnavailable(String),

    /// Gateway-side or transport failure when reaching the TEE (502).
    #[error("TEE error: {0}")]
    TeeError(String),

    /// TEE accepted the relay but returned a client error itself (4xx).
    /// The original status code is passed through so client retry logic
    /// sees the same semantics as if it had called the TEE directly.
    #[error("TEE rejected request (HTTP {status})")]
    TeeRejected { status: u16 },

    /// TEE returned a 5xx other than 503 (e.g. 500/502/504). Passed through
    /// so the client can distinguish "TEE crashed" from "TEE answered with
    /// a timeout" from "TEE is busy" without collapsing all upstream
    /// failures to 502.
    #[error("TEE upstream error (HTTP {status})")]
    TeeUpstreamError { status: u16 },

    /// Client authentication failed (401).
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Rate limit exceeded (429).
    #[error("Rate limit exceeded")]
    RateLimited,

    /// Requested resource not available — e.g. Solana endpoints
    /// when the extension is not enabled (404).
    #[error("Not found: {0}")]
    NotFound(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = match &self {
            GatewayError::TeeUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            GatewayError::TeeError(_) => StatusCode::BAD_GATEWAY,
            GatewayError::TeeRejected { status } | GatewayError::TeeUpstreamError { status } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            GatewayError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            GatewayError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            GatewayError::NotFound(_) => StatusCode::NOT_FOUND,
        };

        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_status_codes() {
        let cases: Vec<(GatewayError, StatusCode)> = vec![
            (
                GatewayError::TeeUnavailable("down".into()),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                GatewayError::TeeError("500".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                GatewayError::Unauthorized("bad key".into()),
                StatusCode::UNAUTHORIZED,
            ),
            (GatewayError::RateLimited, StatusCode::TOO_MANY_REQUESTS),
            (
                GatewayError::NotFound("no".into()),
                StatusCode::NOT_FOUND,
            ),
        ];

        for (error, expected) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), expected);
        }
    }
}
