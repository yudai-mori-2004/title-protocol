// SPDX-License-Identifier: Apache-2.0

//! # Rate Limiter
//!
//! Spec §5.3 -- Client rate limiting.
//!
//! Token bucket per API key. Tokens refill at a constant rate
//! (max_requests / window_secs). Each request consumes one token.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

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

    /// Check if a request from the given API key should be allowed.
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check_rate_limit(&self, api_key: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();

        let bucket = buckets.entry(api_key.to_string()).or_insert(TokenBucket {
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
