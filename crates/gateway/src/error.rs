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
    /// `body` holds the upstream error body (raw bytes; may be JSON or
    /// plain text depending on the TEE handler) so callers can surface
    /// the actual reason instead of a fixed string. Spec §5.3.
    #[error("TEE rejected request (HTTP {status})")]
    TeeRejected { status: u16, body: String },

    /// TEE returned a 5xx other than 503 (e.g. 500/502/504). Passed through
    /// so the client can distinguish "TEE crashed" from "TEE answered with
    /// a timeout" from "TEE is busy" without collapsing all upstream
    /// failures to 502. `body` holds the upstream error body for the same
    /// reason as `TeeRejected::body`.
    #[error("TEE upstream error (HTTP {status})")]
    TeeUpstreamError { status: u16, body: String },

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
            GatewayError::TeeRejected { status, .. }
            | GatewayError::TeeUpstreamError { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            GatewayError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            GatewayError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            GatewayError::NotFound(_) => StatusCode::NOT_FOUND,
        };

        // TEE 経由のエラーは、TEE 側が返した body を `detail` フィールドに
        // 透過する。JSON なら parsed value で展開、それ以外は文字列として。
        // J round3-r3-001 の regression (TEE エラー詳細が固定文字列で潰される)
        // を解消する。
        let body = match &self {
            GatewayError::TeeRejected { body, .. } | GatewayError::TeeUpstreamError { body, .. } => {
                let detail: serde_json::Value = serde_json::from_str(body)
                    .unwrap_or_else(|_| serde_json::Value::String(body.clone()));
                serde_json::json!({ "error": self.to_string(), "detail": detail })
            }
            _ => serde_json::json!({ "error": self.to_string() }),
        };
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
            (GatewayError::NotFound("no".into()), StatusCode::NOT_FOUND),
            (
                GatewayError::TeeRejected {
                    status: 403,
                    body: r#"{"error":"forbidden"}"#.into(),
                },
                StatusCode::FORBIDDEN,
            ),
            (
                GatewayError::TeeUpstreamError {
                    status: 504,
                    body: String::new(),
                },
                StatusCode::GATEWAY_TIMEOUT,
            ),
            // 0 は HTTP status として不正 → from_u16 が None を返し
            // BAD_GATEWAY にフォールバック。これで status field が壊れた
            // 場合の安全側挙動を回帰テストで固定する。
            (
                GatewayError::TeeRejected {
                    status: 0,
                    body: String::new(),
                },
                StatusCode::BAD_GATEWAY,
            ),
        ];

        for (error, expected) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), expected);
        }
    }
}
