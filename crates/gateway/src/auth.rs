// SPDX-License-Identifier: Apache-2.0

//! # API Key Authentication Middleware
//!
//! Spec §5.3 -- Client authentication via API key.
//!
//! Extracts API key from `Authorization: Bearer <key>` header
//! and validates it against the configured key set.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::GatewayError;
use crate::state::GatewayState;

/// Result of probing the `Authorization` header for a Bearer token.
pub(crate) enum AuthHeader {
    /// No `Authorization` header on the request.
    Missing,
    /// Header present and parsed; contains the API key string.
    Bearer(String),
    /// Header present but not a valid `Bearer <utf-8>`.
    Malformed,
}

/// Inspect the `Authorization` header without judging the key itself.
/// Shared between the auth and rate-limit middleware so a malformed header
/// gets the same answer in both places.
pub(crate) fn parse_auth_header(req: &Request) -> AuthHeader {
    let Some(header) = req.headers().get("authorization") else {
        return AuthHeader::Missing;
    };
    let Ok(text) = header.to_str() else {
        return AuthHeader::Malformed;
    };
    match text.strip_prefix("Bearer ") {
        Some(token) if !token.is_empty() => AuthHeader::Bearer(token.to_string()),
        _ => AuthHeader::Malformed,
    }
}

/// API key authentication middleware.
/// Spec §5.3
///
/// Validates the `Authorization: Bearer <key>` header against the
/// configured API key set. Skips authentication for GET /health, and
/// short-circuits when no API keys are configured (development mode).
///
/// Rate limiting is handled by a separate middleware
/// (`crate::rate_limit::rate_limit_middleware`) so that throttling stays
/// active even when authentication is disabled.
pub async fn api_key_auth(
    axum::extract::State(state): axum::extract::State<Arc<GatewayState>>,
    req: Request,
    next: Next,
) -> Result<Response, GatewayError> {
    if req.uri().path() == "/health" && req.method() == axum::http::Method::GET {
        return Ok(next.run(req).await);
    }

    if state.api_keys.is_empty() {
        return Ok(next.run(req).await);
    }

    let key = match parse_auth_header(&req) {
        AuthHeader::Bearer(key) => key,
        AuthHeader::Missing => {
            return Err(GatewayError::Unauthorized(
                "Missing Authorization header".into(),
            ));
        }
        AuthHeader::Malformed => {
            return Err(GatewayError::Unauthorized(
                "Malformed Authorization header".into(),
            ));
        }
    };

    if !state.api_keys.contains(&key) {
        return Err(GatewayError::Unauthorized("Invalid API key".into()));
    }

    Ok(next.run(req).await)
}

/// Configured API key set.
/// Spec §5.3
#[derive(Debug, Clone)]
pub struct ApiKeySet {
    keys: HashSet<String>,
}

impl ApiKeySet {
    pub fn new(keys: Vec<String>) -> Self {
        Self {
            keys: keys.into_iter().collect(),
        }
    }

    pub fn empty() -> Self {
        Self {
            keys: HashSet::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Compare candidate against every configured key with a branchless
    /// XOR accumulator. Length-mismatched entries are skipped, so total
    /// runtime leaks the candidate's length (not which entry matched).
    /// API keys are high-entropy fixed-length tokens, so this leak is
    /// negligible. Never short-circuits on a match.
    pub fn contains(&self, candidate: &str) -> bool {
        let candidate_bytes = candidate.as_bytes();
        let mut matched: u8 = 0;
        for stored in self.keys.iter() {
            let stored_bytes = stored.as_bytes();
            if stored_bytes.len() != candidate_bytes.len() {
                continue;
            }
            let mut diff: u8 = 0;
            for (a, b) in stored_bytes.iter().zip(candidate_bytes.iter()) {
                diff |= a ^ b;
            }
            let is_zero = ((diff as u16).wrapping_sub(1) >> 8) as u8 & 1;
            matched |= is_zero;
        }
        matched != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_set_operations() {
        let set = ApiKeySet::new(vec!["key1".into(), "key2".into()]);
        assert!(!set.is_empty());
        assert!(set.contains("key1"));
        assert!(set.contains("key2"));
        assert!(!set.contains("key3"));
    }

    #[test]
    fn empty_set_allows_all() {
        let set = ApiKeySet::empty();
        assert!(set.is_empty());
        assert!(!set.contains("anything"));
    }

    #[test]
    fn parse_bearer_token() {
        let req = axum::http::Request::builder()
            .header("authorization", "Bearer test-key-123")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(matches!(parse_auth_header(&req), AuthHeader::Bearer(ref s) if s == "test-key-123"));
    }

    #[test]
    fn parse_missing_header() {
        let req = axum::http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(matches!(parse_auth_header(&req), AuthHeader::Missing));
    }

    #[test]
    fn parse_wrong_scheme() {
        let req = axum::http::Request::builder()
            .header("authorization", "Basic dXNlcjpwYXNz")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(matches!(parse_auth_header(&req), AuthHeader::Malformed));
    }

    #[test]
    fn parse_non_utf8_header() {
        let req = axum::http::Request::builder()
            .header(
                "authorization",
                axum::http::HeaderValue::from_bytes(&[0xff, 0xff]).unwrap(),
            )
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(matches!(parse_auth_header(&req), AuthHeader::Malformed));
    }
}
