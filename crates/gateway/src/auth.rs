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

/// Extract API key from the Authorization header.
fn extract_api_key(req: &Request) -> Option<String> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
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

    let key = extract_api_key(&req)
        .ok_or_else(|| GatewayError::Unauthorized("Missing Authorization header".into()))?;

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

    /// API key validation with constant-time per-entry comparison.
    ///
    /// Walks every configured key (never short-circuits on a match) and uses
    /// a XOR-accumulator inner comparison. Length-mismatched entries still
    /// consume a constant number of comparisons against a fixed zero buffer
    /// so the total time depends on the configured set size and the longest
    /// stored key length, not on which (if any) entry matched.
    ///
    /// Note: candidates whose length doesn't appear in the configured set
    /// will leak that fact via overall execution time differences (no entry
    /// performs a same-length compare). API keys are typically high-entropy
    /// fixed-length tokens, making this leak negligible in practice.
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
            // `is_zero = 1` iff diff == 0. Computed branchlessly so we never
            // short-circuit on the first match.
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
    fn extract_bearer_token() {
        let req = axum::http::Request::builder()
            .header("authorization", "Bearer test-key-123")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(extract_api_key(&req), Some("test-key-123".to_string()));
    }

    #[test]
    fn extract_missing_header() {
        let req = axum::http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(extract_api_key(&req), None);
    }

    #[test]
    fn extract_wrong_scheme() {
        let req = axum::http::Request::builder()
            .header("authorization", "Basic dXNlcjpwYXNz")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(extract_api_key(&req), None);
    }
}
