// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Gateway
//!
//! Spec §5.3
//!
//! ## Startup sequence
//!
//! 1. Parse environment variables
//! 2. Initialize HttpTeeClient
//! 3. Build GatewayConfig
//! 4. Start server (TEE info fetch + health check + Axum HTTP)
//!
//! ## Environment variables
//!
//! | Variable | Default | Description |
//! |---|---|---|
//! | TEE_ENDPOINT | http://localhost:4000 | TEE internal HTTP URL |
//! | API_KEYS | (empty = auth disabled) | Comma-separated API keys |
//! | RATE_LIMIT_MAX | 100 | Max requests per window |
//! | RATE_LIMIT_WINDOW_SECS | 60 | Window duration in seconds |
//! | HEALTH_CHECK_INTERVAL_SECS | 10 | TEE health poll interval |
//! | GATEWAY_BIND_ADDR | 0.0.0.0:3000 | Server bind address |

use title_gateway::server::{self, GatewayConfig};
use title_gateway::tee_client::HttpTeeClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Step 1: Parse configuration from environment
    let tee_endpoint = std::env::var("TEE_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4000".to_string());
    tracing::info!(tee_endpoint = %tee_endpoint, "TEE endpoint configured");

    let api_keys: Vec<String> = std::env::var("API_KEYS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect();

    if api_keys.is_empty() {
        tracing::warn!("API_KEYS not set; client authentication disabled (dev mode)");
    } else {
        tracing::info!(count = api_keys.len(), "API keys loaded");
    }

    let rate_limit_max: u32 = std::env::var("RATE_LIMIT_MAX")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let rate_limit_window_secs: u64 = std::env::var("RATE_LIMIT_WINDOW_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let health_check_interval_secs: u64 = std::env::var("HEALTH_CHECK_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let bind_addr = std::env::var("GATEWAY_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    // Step 2: Initialize HttpTeeClient
    let tee_client = HttpTeeClient::new(tee_endpoint);

    // Step 3: Build config
    let config = GatewayConfig {
        bind_addr,
        tee_client: Box::new(tee_client),
        api_keys,
        rate_limit_max,
        rate_limit_window_secs,
        health_check_interval_secs,
    };

    // Step 4: Start server
    server::run(config).await
}
