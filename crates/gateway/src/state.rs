// SPDX-License-Identifier: Apache-2.0

//! # Gateway Shared State
//!
//! Spec §5.3
//!
//! Holds TEE client, cached TEE info, API key set, and rate limiter.
//! The TEE info cache is refreshed on startup and when TEE restart
//! is detected.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::auth::ApiKeySet;
use crate::rate_limit::RateLimiter;
use crate::tee_client::{TeeClient, TeeClientError};
use crate::{KeysResponse, ProcessorsResponse, SolanaKeysResponse};

// ---------------------------------------------------------------------------
// TEE info cache
// ---------------------------------------------------------------------------

/// Cached TEE state.
/// Spec §5.3 -- Gateway caches TEE info and refreshes on restart.
#[derive(Debug, Clone)]
pub struct TeeInfoCache {
    pub keys: Option<KeysResponse>,
    pub processors: Option<ProcessorsResponse>,
    pub tee_type: Option<String>,
    pub solana_keys: Option<SolanaKeysResponse>,
}

impl TeeInfoCache {
    fn empty() -> Self {
        Self {
            keys: None,
            processors: None,
            tee_type: None,
            solana_keys: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Gateway state
// ---------------------------------------------------------------------------

/// Gateway shared state.
/// Spec §5.3
pub struct GatewayState {
    pub tee_client: Box<dyn TeeClient>,
    pub tee_cache: RwLock<TeeInfoCache>,
    pub tee_available: AtomicBool,
    pub api_keys: ApiKeySet,
    pub rate_limiter: RateLimiter,
}

impl GatewayState {
    pub fn new(
        tee_client: Box<dyn TeeClient>,
        api_keys: ApiKeySet,
        rate_limiter: RateLimiter,
    ) -> Self {
        Self {
            tee_client,
            tee_cache: RwLock::new(TeeInfoCache::empty()),
            tee_available: AtomicBool::new(false),
            api_keys,
            rate_limiter,
        }
    }

    /// Refresh cached TEE info by querying all TEE endpoints.
    /// Spec §5.3 -- Called on startup and when TEE restart is detected.
    pub async fn refresh_tee_info(&self) -> Result<(), TeeClientError> {
        let health = self.tee_client.health().await?;
        let keys = self.tee_client.keys().await?;
        let processors = self.tee_client.processors().await?;
        let solana_keys = self.tee_client.solana_keys().await?;

        let mut cache = self.tee_cache.write().await;
        cache.keys = Some(keys);
        cache.processors = Some(processors);
        cache.tee_type = health.tee_type.clone();
        cache.solana_keys = solana_keys;

        self.tee_available.store(true, Ordering::Release);
        tracing::info!(tee_type = ?health.tee_type, "TEE info refreshed");
        Ok(())
    }

    /// Check TEE health and refresh cache if TEE restarted.
    /// Spec §5.3 -- TEE restart detection + key refresh.
    ///
    /// Restart is detected by comparing cached keys with live keys.
    /// TEE generates fresh keys on each boot, so a key change means restart.
    pub async fn check_and_refresh(&self) {
        match self.tee_client.health().await {
            Ok(_health) => {
                let was_unavailable = !self.tee_available.load(Ordering::Acquire);

                let keys_changed = match self.tee_client.keys().await {
                    Ok(live_keys) => {
                        let cache = self.tee_cache.read().await;
                        cache.keys.as_ref() != Some(&live_keys)
                    }
                    Err(_) => false,
                };

                if was_unavailable || keys_changed {
                    tracing::info!("TEE restart detected, refreshing info");
                    if let Err(e) = self.refresh_tee_info().await {
                        tracing::error!(error = %e, "Failed to refresh TEE info");
                    }
                } else {
                    self.tee_available.store(true, Ordering::Release);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "TEE health check failed");
                self.tee_available.store(false, Ordering::Release);
            }
        }
    }

    pub fn is_tee_available(&self) -> bool {
        self.tee_available.load(Ordering::Acquire)
    }
}

/// Spawn a background task that periodically checks TEE health.
/// Spec §5.3 -- TEE restart detection.
pub fn spawn_health_check(state: Arc<GatewayState>, interval_secs: u64) {
    tokio::spawn(async move {
        let interval = tokio::time::Duration::from_secs(interval_secs);
        loop {
            state.check_and_refresh().await;
            tokio::time::sleep(interval).await;
        }
    });
}
