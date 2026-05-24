// SPDX-License-Identifier: Apache-2.0

//! # Gateway Error Type
//!
//! Spec §5.3
//!
//! Maps gateway errors to HTTP status codes via Axum's IntoResponse.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Gateway error.
/// Spec §5.3
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// TEE is not reachable or not ready (503).
    #[error("TEE unavailable: {0}")]
    TeeUnavailable(String),

    /// TEE returned an error (502).
    #[error("TEE error: {0}")]
    TeeError(String),

    /// Client authentication failed (401).
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Rate limit exceeded (429).
    #[error("Rate limit exceeded")]
    RateLimited,

    /// Requested resource not available -- e.g. Solana endpoints
    /// when extension is not enabled (404).
    #[error("Not found: {0}")]
    NotFound(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = match &self {
            GatewayError::TeeUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            GatewayError::TeeError(_) => StatusCode::BAD_GATEWAY,
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
