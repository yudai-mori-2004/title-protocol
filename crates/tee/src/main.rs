// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol TEE Server
//!
//! Spec §5.2
//!
//! ## Startup sequence
//!
//! 1. Select runtime (TEE_RUNTIME env: "mock" or "nitro")
//! 2. Generate encryption key bundle (x25519, p256, ml-kem-768)
//! 3. Generate Solana Extension signing key (Ed25519)
//! 4. Register processors
//! 5. Initialize ResourcePool
//! 6. Start Axum HTTP server on 0.0.0.0:4000

use std::sync::Arc;
use std::time::Instant;

use title_core::{C2paVerifyProcessor, ProcessorRegistry};
use title_crypto::key_bundle::KeyBundle;
use title_solana::signing_key::SolanaSigningKey;
use title_tee::content_fetch::HttpContentFetcher;
use title_tee::resource_pool::ResourcePool;
use title_tee::runtime::mock::MockRuntime;
use title_tee::server::{router, TeeAppState};
use title_tee::TeeRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Step 1: Runtime selection
    // Spec §5.2
    let runtime_name = std::env::var("TEE_RUNTIME").unwrap_or_else(|_| "mock".to_string());
    let runtime: Box<dyn TeeRuntime> = match runtime_name.as_str() {
        "mock" => {
            tracing::info!("Starting with MockRuntime");
            Box::new(MockRuntime::new())
        }
        #[cfg(feature = "vendor-aws")]
        "nitro" => {
            tracing::info!("Starting with NitroRuntime");
            Box::new(
                title_tee::vendor::aws::NitroRuntime::new()
                    .expect("Failed to initialize NitroRuntime"),
            )
        }
        other => {
            #[allow(unused_mut)]
            let mut supported = vec!["mock"];
            #[cfg(feature = "vendor-aws")]
            supported.push("nitro");
            return Err(format!(
                "Unsupported TEE_RUNTIME: {other} (supported: {})",
                supported.join(", ")
            )
            .into());
        }
    };

    // Step 2: Generate encryption key bundle
    // Spec §2.4 — per-suite key pairs, lost on restart
    tracing::info!("Generating encryption key bundle...");
    let key_bundle = KeyBundle::generate(&mut rand::rngs::OsRng)?;
    tracing::info!("Encryption key bundle ready (x25519, p256, ml-kem-768)");

    // Step 3: Generate Solana Extension signing key
    // Spec §6.2 — Ed25519 keypair for cNFT partial signing
    let solana_key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
    tracing::info!(
        solana_pubkey = %solana_key.pubkey_base58(),
        "Solana signing key generated"
    );

    // Step 4: Register processors
    // Spec §3.1 — processors are compiled into the TEE binary
    let mut registry = ProcessorRegistry::new();
    registry.register(Box::new(C2paVerifyProcessor::new()));
    tracing::info!(
        processors = ?registry.processor_ids(),
        "Processors registered"
    );

    // Step 5: Initialize ResourcePool
    // Spec §4.1 — two-tier admission control
    let total_limit: usize = std::env::var("POOL_TOTAL_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512 * 1024 * 1024); // 512 MB default
    let admission_limit: usize = std::env::var("POOL_ADMISSION_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(total_limit * 3 / 4);
    let pool = Arc::new(ResourcePool::new(admission_limit, total_limit));
    tracing::info!(admission_limit, total_limit, "ResourcePool initialized");

    // Build HTTP content fetcher
    // Must be constructed outside async context because reqwest::blocking::Client
    // creates its own tokio runtime internally.
    let fetcher = tokio::task::spawn_blocking(|| HttpContentFetcher::new())
        .await
        .expect("Failed to build HTTP content fetcher");

    // Build application state
    let state = Arc::new(TeeAppState {
        runtime,
        key_bundle,
        solana_key,
        registry,
        pool,
        fetcher: Box::new(fetcher),
        started_at: Instant::now(),
    });

    let app = router(state);

    // Step 6: Start Axum HTTP server
    let bind_addr = std::env::var("TEE_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:4000".to_string());
    tracing::info!(addr = %bind_addr, "TEE server starting");

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("TEE server shut down");
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    tracing::info!("Received shutdown signal");
}
