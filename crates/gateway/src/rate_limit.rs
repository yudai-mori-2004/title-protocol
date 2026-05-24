// SPDX-License-Identifier: Apache-2.0

//! # Rate Limiter
//!
//! Spec §5.3 -- Client rate limiting.
//!
//! Token bucket per identity (typically the API key, or a shared anonymous
//! bucket when API-key auth is disabled). Tokens refill at a constant rate
//! (max_requests / window_secs). Each request consumes one token.
//!
//! Rate limiting runs **independently of authentication**: even when no
//! API keys are configured the anonymous bucket still throttles requests,
//! so a buggy client cannot stampede the TEE.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::GatewayError;
use crate::state::GatewayState;

/// Identity used when an unauthenticated request hits the rate limiter.
/// All such requests share a single token bucket.
pub const ANONYMOUS_IDENTITY: &str = "__anonymous__";

/// Per-API-key rate limiter using token bucket algorithm.
/// Spec §5.3
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    max_tokens: f64,
    refill_rate: f64,
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a rate limiter.
    ///
    /// - `max_requests`: Maximum requests per window
    /// - `window_secs`: Window duration in seconds
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        let max_tokens = max_requests as f64;
        let refill_rate = max_tokens / window_secs as f64;
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_tokens,
            refill_rate,
        }
    }

    /// Check if a request from the given identity should be allowed.
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check_rate_limit(&self, identity: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();

        let bucket = buckets.entry(identity.to_string()).or_insert(TokenBucket {
            tokens: self.max_tokens,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Rate-limiting middleware. Applied to every request except `GET /health`.
///
/// The identity used for the token bucket is the Bearer API key if one is
/// present on the request; otherwise the shared anonymous bucket. Runs
/// independently of API-key validation so that a deployment with no
/// `API_KEYS` configured still rejects runaway traffic.
pub async fn rate_limit_middleware(
    axum::extract::State(state): axum::extract::State<Arc<GatewayState>>,
    req: Request,
    next: Next,
) -> Result<Response, GatewayError> {
    if req.uri().path() == "/health" && req.method() == axum::http::Method::GET {
        return Ok(next.run(req).await);
    }

    let identity = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .unwrap_or_else(|| ANONYMOUS_IDENTITY.to_string());

    if !state.rate_limiter.check_rate_limit(&identity) {
        return Err(GatewayError::RateLimited);
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn allows_within_limit() {
        let limiter = RateLimiter::new(5, 1);
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("key1"));
        }
    }

    #[test]
    fn rejects_over_limit() {
        let limiter = RateLimiter::new(3, 1);
        assert!(limiter.check_rate_limit("key1"));
        assert!(limiter.check_rate_limit("key1"));
        assert!(limiter.check_rate_limit("key1"));
        assert!(!limiter.check_rate_limit("key1"));
    }

    #[test]
    fn refills_over_time() {
        let limiter = RateLimiter::new(2, 1);
        assert!(limiter.check_rate_limit("key1"));
        assert!(limiter.check_rate_limit("key1"));
        assert!(!limiter.check_rate_limit("key1"));

        thread::sleep(Duration::from_millis(600));
        assert!(limiter.check_rate_limit("key1"));
    }

    #[test]
    fn independent_per_key() {
        let limiter = RateLimiter::new(2, 1);
        assert!(limiter.check_rate_limit("key1"));
        assert!(limiter.check_rate_limit("key1"));
        assert!(!limiter.check_rate_limit("key1"));

        // key2 has its own bucket
        assert!(limiter.check_rate_limit("key2"));
        assert!(limiter.check_rate_limit("key2"));
        assert!(!limiter.check_rate_limit("key2"));
    }
}
