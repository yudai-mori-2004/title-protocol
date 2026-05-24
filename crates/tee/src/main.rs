// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol TEE Server
//!
//! Spec §5.2 startup sequence:
//!
//! 1. Select TeeRuntime + paired AttestationVerifier
//! 2. Generate encryption key bundle and Solana signing key from TEE entropy
//! 3. Self-attest — capture this TEE's measurement; failure aborts boot
//! 4. Capture the registration attestation that binds the Solana pubkey
//! 5. Register processors and allocate the ResourcePool
//! 6. Construct the outbound content fetcher (direct or proxy-mediated)
//! 7. Start the Axum HTTP server on `TEE_BIND_ADDR` (default 0.0.0.0:4000)

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

    // Step 3: Self-attestation — bind the TEE's measurement before any other
    // setup so a failure aborts boot rather than letting measurement
    // enforcement silently degrade to a no-op.
    let now_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| format!("System time before UNIX epoch: {e}"))?;
    let self_attestation = runtime
        .get_attestation_document(&[])
        .map_err(|e| format!("Failed to obtain self-attestation: {e}"))?;
    let verified_self = attestation_verifier
        .verify(&self_attestation, now_unix_secs)
        .map_err(|e| format!("Self-attestation verification failed: {e}"))?;
    let expected_measurement = verified_self.measurement.into_boxed_slice();
    tracing::info!(
        tee_type = runtime.tee_type(),
        measurement = %hex::encode(&expected_measurement[..expected_measurement.len().min(8)]),
        measurement_len = expected_measurement.len(),
        "Self-attestation measurement captured"
    );

    // Step 4: Registration attestation. Spec §6.2 — `user_data =
    // SHA-256(solana_pubkey)`; consumed by the off-host SP1 prover to produce
    // the Groth16 proof that unlocks `register_key` on Solana.
    let solana_pubkey_hash = solana_key.pubkey_hash();
    let registration_attestation = runtime
        .get_attestation_document(&solana_pubkey_hash)
        .map_err(|e| format!("Failed to obtain registration attestation: {e}"))?;
    tracing::info!(
        bytes = registration_attestation.len(),
        "Registration attestation captured (user_data = SHA-256(solana_pubkey))"
    );

    // Step 5: Processors + ResourcePool. Spec §3.1 / §4.1.
    let mut registry = ProcessorRegistry::new();
    registry.register(Box::new(C2paVerifyProcessor::new()));
    tracing::info!(
        processors = ?registry.processor_ids(),
        "Processors registered"
    );

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

    // Step 6: Outbound content fetcher. Spec §5.2.
    //   PROXY_ADDR=direct (or unset) → reqwest direct (dev / mock runtimes)
    //   PROXY_ADDR=vsock://CID:PORT  → vsock to title-proxy (Nitro production)
    //   PROXY_ADDR=HOST:PORT         → TCP to title-proxy (dev with real proxy)
    //
    // Built via spawn_blocking because reqwest::blocking::Client constructs
    // its own tokio runtime, which panics if done inside an async context.
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
        registration_attestation,
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

