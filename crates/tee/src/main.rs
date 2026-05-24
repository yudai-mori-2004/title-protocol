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

use title_attestation::AttestationVerifier;
use title_core::{C2paVerifyProcessor, ProcessorRegistry};
use title_crypto::key_bundle::KeyBundle;
use title_solana::signing_key::SolanaSigningKey;
use title_tee::content_fetch::{ContentFetcher, HttpContentFetcher};
use title_tee::proxy_fetcher::{ProxyContentFetcher, ProxyEndpoint};
use title_tee::resource_pool::ResourcePool;
use title_tee::server::{router, TeeAppState};
use title_tee::TeeRuntime;

#[cfg(feature = "runtime-mock")]
use title_attestation::MockAttestationVerifier;
#[cfg(feature = "runtime-mock")]
use title_tee::runtime::mock::MockRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Step 1: Runtime + matching Attestation verifier selection.
    // Spec §5.2, §6.2 — every TeeRuntime ships paired with the
    // AttestationVerifier that knows how to authenticate documents from that
    // runtime; the pairing is locked at compile time via Cargo features.
    let runtime_name = std::env::var("TEE_RUNTIME").unwrap_or_else(|_| {
        if cfg!(feature = "runtime-mock") {
            "mock".to_string()
        } else {
            "nitro".to_string()
        }
    });
    let (runtime, attestation_verifier): (
        Box<dyn TeeRuntime>,
        Box<dyn AttestationVerifier + Send + Sync>,
    ) = match runtime_name.as_str() {
        #[cfg(feature = "runtime-mock")]
        "mock" => {
            tracing::info!("Starting with MockRuntime");
            (
                Box::new(MockRuntime::new()),
                Box::new(MockAttestationVerifier::new()),
            )
        }
        #[cfg(feature = "vendor-aws")]
        "nitro" => {
            tracing::info!("Starting with NitroRuntime");
            (
                Box::new(title_tee::vendor::aws::NitroRuntime::new()?),
                Box::new(title_attestation_aws_nitro::AwsNitroVerifier::new()),
            )
        }
        other => {
            #[allow(unused_mut)]
            let mut supported: Vec<&str> = Vec::new();
            #[cfg(feature = "runtime-mock")]
            supported.push("mock");
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
    // Spec §2.4, §5.2 — per-suite key pairs, lost on restart. Entropy comes
    // from the TEE hardware via `TeeRuntime::random_bytes` (NSM GetRandom on
    // Nitro). Using the host kernel's `OsRng` directly would defeat the
    // point: enclave-internal entropy must be vendor-attestable, and Nitro's
    // /dev/urandom has no guaranteed seed source other than NSM.
    tracing::info!("Generating encryption key bundle from TEE entropy...");
    let mut key_bundle_rng = tee_seeded_rng(runtime.as_ref(), "key_bundle")?;
    let key_bundle = KeyBundle::generate(&mut key_bundle_rng)?;
    tracing::info!("Encryption key bundle ready (x25519, p256, ml-kem-768)");

    // Step 3: Generate Solana Extension signing key
    // Spec §6.2 — Ed25519 keypair for cNFT partial signing. Same entropy
    // requirement as the encryption bundle above.
    let mut solana_rng = tee_seeded_rng(runtime.as_ref(), "solana_signing_key")?;
    let solana_key = SolanaSigningKey::generate(&mut solana_rng);
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

    // Build the content fetcher. Spec §5.2 — selection rules:
    //   PROXY_ADDR=direct (or unset) → reqwest direct (dev / mock runtimes)
    //   PROXY_ADDR=vsock://CID:PORT   → vsock to title-proxy (production / Nitro)
    //   PROXY_ADDR=HOST:PORT          → TCP to title-proxy   (dev with real proxy)
    //
    // Built outside the async runtime because reqwest::blocking::Client spawns
    // its own tokio runtime internally; doing so inside an async context panics.
    let proxy_addr = std::env::var("PROXY_ADDR").unwrap_or_else(|_| "direct".to_string());
    let fetcher: Box<dyn ContentFetcher> = if proxy_addr == "direct" {
        tracing::info!("Content fetcher: direct (reqwest)");
        let f = tokio::task::spawn_blocking(HttpContentFetcher::new)
            .await
            .expect("Failed to build HTTP content fetcher");
        Box::new(f)
    } else {
        tracing::info!(addr = %proxy_addr, "Content fetcher: proxy-mediated");
        let endpoint = ProxyEndpoint::parse(&proxy_addr).map_err(|e| {
            format!("Invalid PROXY_ADDR={proxy_addr}: {e}")
        })?;
        Box::new(ProxyContentFetcher::new(endpoint))
    };

    // Step 5.5: Capture this TEE's own measurement by self-attesting.
    // Spec §5.2 — without the self-attestation, downstream measurement checks
    // would silently fall back to "no enforcement", so failure here is fatal.
    let now_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| format!("System time before UNIX epoch: {e}"))?;
    let self_attestation = runtime
        .get_attestation_document(&[])
        .map_err(|e| format!("Failed to obtain self-attestation: {e}"))?;
    let verified_self = attestation_verifier
        .verify(&self_attestation, now_unix_secs)
        .map_err(|e| {
            format!(
                "Self-attestation failed; refusing to start because measurement \
                 enforcement would silently degrade: {e}"
            )
        })?;
    let expected_measurement = verified_self.measurement;
    tracing::info!(
        measurement = %hex_short(&expected_measurement),
        "Self-attestation measurement captured"
    );

    // Build application state
    let state = Arc::new(TeeAppState {
        runtime,
        key_bundle,
        solana_key,
        registry,
        pool,
        fetcher,
        attestation_verifier,
        expected_measurement,
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

/// Build a deterministic CryptoRng seeded with bytes from the TEE hardware.
///
/// The runtime's `random_bytes` call routes to NSM GetRandom on AWS Nitro
/// (or the equivalent vendor entropy source). We then wrap those 32 bytes
/// in a ChaCha20 PRNG so callers can pass `&mut impl CryptoRng + RngCore`
/// to existing key-generation APIs without each one needing to know about
/// NSM. `purpose` is included only in error messages for debuggability.
fn tee_seeded_rng(
    runtime: &dyn TeeRuntime,
    purpose: &str,
) -> Result<rand_chacha::ChaCha20Rng, Box<dyn std::error::Error>> {
    use rand::SeedableRng;
    let seed_bytes = runtime
        .random_bytes(32)
        .map_err(|e| format!("TEE entropy ({purpose}) unavailable: {e}"))?;
    let seed: [u8; 32] = seed_bytes
        .try_into()
        .map_err(|v: Vec<u8>| format!("expected 32 entropy bytes, got {}", v.len()))?;
    Ok(rand_chacha::ChaCha20Rng::from_seed(seed))
}

/// Render the first 8 bytes of a measurement as hex, for log lines that need
/// a stable but compact identifier (full PCR0 is 48 bytes).
fn hex_short(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut s = String::with_capacity(take * 2 + 4);
    for b in &bytes[..take] {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    if bytes.len() > take {
        s.push_str("..");
    }
    s
}
