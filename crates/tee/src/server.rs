// SPDX-License-Identifier: Apache-2.0

//! # TEE HTTP Server
//!
//! Spec §2.5, §5.2
//!
//! Axum-based internal HTTP server running inside the TEE.
//! Exposes endpoints for the Gateway to call:
//!
//! - `GET /health` — uptime and runtime info
//! - `GET /keys` — encryption public keys (KeyBundle)
//! - `GET /processors` — registered processor IDs
//! - `POST /process` — core processing pipeline
//! - `GET /solana-keys` — Solana Extension public key
//! - `POST /extension/solana` — Solana Extension cNFT mint

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use title_attestation::AttestationVerifier;
use title_core::{ProcessRequest, ProcessorRegistry};
use title_crypto::key_bundle::KeyBundle;
use title_solana::extension::{self, ExtensionRequest};
use title_solana::signing_key::SolanaSigningKey;

use crate::content_fetch::ContentFetcher;
use crate::orchestrator;
use crate::resource_pool::ResourcePool;
use crate::TeeRuntime;

// ---------------------------------------------------------------------------
// Application State
// ---------------------------------------------------------------------------

/// TEE application state shared across all request handlers.
/// Spec §5.2
pub struct TeeAppState {
    /// TEE hardware runtime (mock or vendor-specific).
    pub runtime: Box<dyn TeeRuntime>,
    /// Encryption key bundle (x25519 + p256 + ml-kem-768).
    /// Spec §2.4 — generated at startup, lost on restart.
    pub key_bundle: KeyBundle,
    /// Solana Extension signing key.
    /// Spec §6.2 — Ed25519 keypair for cNFT partial signing.
    pub solana_key: SolanaSigningKey,
    /// Processor registry with all registered processors.
    pub registry: ProcessorRegistry,
    /// Memory management pool.
    /// Spec §4.1
    pub pool: Arc<ResourcePool>,
    /// HTTP content fetcher for external storage (`/process` 用、`max_body_bytes
    /// = 100 MiB`)。
    pub fetcher: Box<dyn ContentFetcher>,
    /// `/extension/solana` 専用の fetcher。offchain data は metadata + hash の
    /// JSON だけで実体は数 KiB 〜 数百 KiB に収まる想定なので、fetcher 自体の
    /// `max_body_bytes` を `MAX_OFFCHAIN_DATA_BYTES = 1 MiB` に絞っておく。
    /// これにより 100 MiB のレスポンスでメモリを取られる経路を物理的に塞ぐ。
    pub extension_fetcher: Box<dyn ContentFetcher>,
    /// Vendor-specific Attestation Document verifier (Spec §6.2).
    /// Mock runtime pairs with `MockAttestationVerifier`; Nitro pairs with
    /// `AwsNitroVerifier`. Used by the Solana Extension to authenticate
    /// off-chain data before partial signing.
    pub attestation_verifier: Box<dyn AttestationVerifier + Send + Sync>,
    /// The TEE's own measurement, captured at boot via self-attestation.
    /// Length is vendor-defined (AWS Nitro = 48-byte SHA-384 PCR0).
    /// Spec §6.2 — must match the registered measurement on-chain.
    pub expected_measurement: Box<[u8]>,
    /// Attestation Document binding the Solana signing key to the TEE.
    /// Spec §6.2 — captured at boot with `user_data = SHA-256(solana_pubkey)`.
    /// Exposed via `GET /solana-keys` for off-host SP1 prover consumption.
    pub registration_attestation: Vec<u8>,
    /// Server start time for uptime calculation.
    pub started_at: Instant,
}

/// Build the TEE Axum router.
/// Spec §2.5
pub fn router(state: Arc<TeeAppState>) -> Router {
    use axum::extract::DefaultBodyLimit;

    // ProcessRequest / SolanaExtensionBody are pure metadata JSON — content
    // bytes come from `fetcher`, not the request body. Cap the JSON envelope
    // at 64 KiB so a runaway Gateway can't push a 100-MB document into the
    // TEE before admission control sees the request.
    let post_limit = DefaultBodyLimit::max(64 * 1024);

    Router::new()
        .route("/health", get(handle_health))
        .route("/keys", get(handle_keys))
        .route("/processors", get(handle_processors))
        .route("/process", post(handle_process).layer(post_limit))
        .route("/solana-keys", get(handle_solana_keys))
        .route(
            "/extension/solana",
            post(handle_solana_extension).layer(post_limit),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /health — TEE health status.
/// Spec §2.5
async fn handle_health(State(state): State<Arc<TeeAppState>>) -> impl IntoResponse {
    let uptime = state.started_at.elapsed().as_secs();
    Json(serde_json::json!({
        "status": "ok",
        "tee_type": state.runtime.tee_type(),
        "uptime_secs": uptime,
    }))
}

/// GET /keys — encryption public keys.
/// Spec §2.4, §2.5
async fn handle_keys(State(state): State<Arc<TeeAppState>>) -> impl IntoResponse {
    Json(serde_json::json!({
        "keys": state.key_bundle.public_keys(),
    }))
}

/// GET /processors — registered processor IDs.
/// Spec §2.5
async fn handle_processors(State(state): State<Arc<TeeAppState>>) -> impl IntoResponse {
    Json(serde_json::json!({
        "processors": state.registry.processor_ids(),
    }))
}

/// POST /process — core processing pipeline.
/// Spec §2.5, §5.2
///
/// Plaintext requests get a JSON `ProcessResponse`. Encrypted requests
/// (§2.4) get back `application/octet-stream` containing the
/// `nonce || ciphertext` wire payload; only the requesting client holds
/// the response key needed to decrypt it.
async fn handle_process(
    State(state): State<Arc<TeeAppState>>,
    Json(request): Json<ProcessRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let result = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || {
            orchestrator::process_request(
                &request,
                state.fetcher.as_ref(),
                &state.registry,
                state.runtime.as_ref(),
                &state.pool,
                &state.key_bundle,
            )
        }
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Task panicked: {e}") })),
        )
    })?;

    match result {
        Ok(orchestrator::ProcessOutcome::Plaintext(response)) => Ok(Json(response).into_response()),
        Ok(orchestrator::ProcessOutcome::Encrypted(bytes)) => {
            use axum::http::header;
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/octet-stream")],
                bytes,
            )
                .into_response())
        }
        Err(e) => Err(orchestrator_error_to_response(e)),
    }
}

/// Map orchestrator errors to HTTP status codes. Gateway-side retry logic
/// keys off these codes, so the buckets matter:
/// - 5xx: TEE / upstream failure, client may retry.
/// - 502: the upstream URL the client provided is unreachable / malformed.
/// - 503: admission control rejected the request, retry later.
/// - 4xx: client supplied something the TEE cannot accept; do not retry.
fn orchestrator_error_to_response(
    e: orchestrator::OrchestratorError,
) -> (StatusCode, Json<serde_json::Value>) {
    use orchestrator::OrchestratorError as O;
    let status = match &e {
        O::AdmissionRejected => StatusCode::SERVICE_UNAVAILABLE,
        O::FetchFailed(_) => StatusCode::BAD_GATEWAY,
        O::AttestationFailed(_) | O::JcsFailed(_) | O::JsonError(_) | O::ResponseSealFailed(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        O::EncryptionRequiresSingleInput
        | O::EncryptionSuiteMismatch { .. }
        | O::PayloadMetadataInvalid(_)
        | O::SignatureHashMismatch
        | O::DecryptionFailed(_)
        | O::SignatureHashFailed(_) => StatusCode::BAD_REQUEST,
    };
    let body = if status == StatusCode::SERVICE_UNAVAILABLE {
        Json(serde_json::json!({ "error": "Service busy, try again later" }))
    } else {
        Json(serde_json::json!({ "error": e.to_string() }))
    };
    (status, body)
}

/// GET /solana-keys — Solana Extension public key + registration attestation.
/// Spec §2.5, §6.2
///
/// `registration_attestation_b64` is the Attestation Document captured at
/// TEE boot with `user_data = SHA-256(solana_pubkey)`. Operators feed it to
/// the off-host SP1 prover; the resulting Groth16 proof is what unlocks
/// `register_key` on Solana (spec §6.2 step 3). Empty string when the
/// runtime doesn't produce one (mock).
async fn handle_solana_keys(State(state): State<Arc<TeeAppState>>) -> impl IntoResponse {
    use base64::Engine;
    let attestation_b64 = if state.registration_attestation.is_empty() {
        String::new()
    } else {
        base64::engine::general_purpose::STANDARD.encode(&state.registration_attestation)
    };
    Json(serde_json::json!({
        "solana_pubkey": state.solana_key.pubkey_base58(),
        "registration_attestation_b64": attestation_b64,
    }))
}

/// Solana Extension request body.
/// Spec §6.2
#[derive(serde::Deserialize)]
struct SolanaExtensionBody {
    offchain_data_url: String,
    payer: String,
    merkle_tree: String,
    recent_blockhash: String,
    #[serde(default)]
    collection: Option<String>,
}

/// POST /extension/solana — Solana Extension cNFT mint.
/// Spec §2.5, §6.2
async fn handle_solana_extension(
    State(state): State<Arc<TeeAppState>>,
    Json(body): Json<SolanaExtensionBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // システム時計の取得は fail-fast。後段 (attestation verify) で必須なので、
    // admission ticket や fetch を走らせる前に弾く。
    let now_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("System clock unavailable: {e}")
                })),
            )
        })?;

    let ext_request = ExtensionRequest::from_strings(
        &body.offchain_data_url,
        body.collection.as_deref(),
        &body.merkle_tree,
        &body.recent_blockhash,
        &body.payer,
    )
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    // Offchain data の物理サイズ強制を 2 層構成にする:
    //   (a) admission ticket で MAX_OFFCHAIN_DATA_BYTES を即時 extend し、
    //       pool.used に積む。これにより N 並列リクエストは admission_limit
    //       までで頭打ちになる (同時に乗せられる総 RAM が物理的に制限される)。
    //   (b) `extension_fetcher` 自身の `max_body_bytes` も
    //       MAX_OFFCHAIN_DATA_BYTES に絞ってあるので、fetcher がストリーミング
    //       中に超過バイトを reject する。post-fetch チェックの「100 MiB まで
    //       RAM に乗ったあと弾く」という従来の弱点が塞がれる。
    // 仕様 §4.1 / §4.2 の memory budget を `/extension/solana` も同じ枠で守る。
    const MAX_OFFCHAIN_DATA_BYTES: usize = 1024 * 1024;
    let mut ticket = state
        .pool
        .try_admit(Some(MAX_OFFCHAIN_DATA_BYTES as u64))
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Service busy, try again later" })),
            )
        })?;
    ticket
        .extend(MAX_OFFCHAIN_DATA_BYTES)
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": format!("Extension reserve failed: {e}")
                })),
            )
        })?;

    let offchain_resp = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        let url = body.offchain_data_url.clone();
        move || state.extension_fetcher.fetch(&url)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("fetch task panicked: {e}") })),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Offchain data fetch failed: {e}") })),
        )
    })?;

    // fetcher 内部の `max_body_bytes` で既に弾かれている想定だが、安全側の
    // 確認として最終長さを再チェックする。
    if offchain_resp.body.len() > MAX_OFFCHAIN_DATA_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": format!(
                    "offchain data too large: {} bytes (max {MAX_OFFCHAIN_DATA_BYTES})",
                    offchain_resp.body.len()
                )
            })),
        ));
    }
    drop(ticket);

    let offchain_data: title_core::ProcessResponse = serde_json::from_slice(&offchain_resp.body)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid offchain data: {e}") })),
            )
        })?;

    let tx_bytes = extension::process_extension(
        state.attestation_verifier.as_ref(),
        &state.solana_key,
        &offchain_data,
        &ext_request,
        Some(&state.expected_measurement),
        now_unix_secs,
    )
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &tx_bytes);

    Ok(Json(serde_json::json!({
        "partial_tx": encoded,
    })))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_fetch::{FetchError, FetchResponse};
    use crate::runtime::mock::MockRuntime;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct MockFetcher {
        responses: HashMap<String, (Vec<u8>, Option<String>)>,
    }

    impl MockFetcher {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }

        fn add(&mut self, url: &str, body: Vec<u8>, content_type: Option<&str>) {
            self.responses
                .insert(url.to_string(), (body, content_type.map(|s| s.to_string())));
        }
    }

    impl ContentFetcher for MockFetcher {
        fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError> {
            let (body, ct) = self.responses.get(url).ok_or(FetchError::HttpStatus {
                status: 404,
                url: url.to_string(),
            })?;
            Ok(FetchResponse {
                body: body.clone(),
                content_type: ct.clone(),
                etag: None,
            })
        }
    }

    fn test_state() -> Arc<TeeAppState> {
        test_state_with_fetcher(MockFetcher::new())
    }

    fn test_state_with_fetcher(fetcher: MockFetcher) -> Arc<TeeAppState> {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(title_core::C2paVerifyProcessor::new()));

        Arc::new(TeeAppState {
            runtime: Box::new(MockRuntime::new()),
            key_bundle: KeyBundle::generate(&mut rand::rngs::OsRng).unwrap(),
            solana_key: SolanaSigningKey::generate(&mut rand::rngs::OsRng),
            registry,
            pool: Arc::new(ResourcePool::with_single_limit(1_000_000_000)),
            fetcher: Box::new(fetcher.clone()),
            extension_fetcher: Box::new(fetcher),
            attestation_verifier: Box::new(title_attestation::MockAttestationVerifier::new()),
            expected_measurement: title_attestation::MockAttestationVerifier::MEASUREMENT
                .to_vec()
                .into_boxed_slice(),
            registration_attestation: Vec::new(),
            started_at: Instant::now(),
        })
    }

    async fn get(app: &Router, path: &str) -> (StatusCode, serde_json::Value) {
        let req = axum::http::Request::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    async fn post_json(
        app: &Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    // ---- GET /health ----

    #[tokio::test]
    async fn health_returns_ok() {
        let state = test_state();
        let app = router(state);
        let (status, json) = get(&app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["tee_type"], "mock");
        assert!(json["uptime_secs"].is_number());
    }

    // ---- GET /keys ----

    #[tokio::test]
    async fn keys_returns_all_suites() {
        let state = test_state();
        let app = router(state);
        let (status, json) = get(&app, "/keys").await;
        assert_eq!(status, StatusCode::OK);
        let keys = json["keys"].as_object().unwrap();
        assert!(keys.contains_key("x25519"));
        assert!(keys.contains_key("p256"));
        assert!(keys.contains_key("ml-kem-768"));
    }

    // ---- GET /processors ----

    #[tokio::test]
    async fn processors_returns_registered() {
        let state = test_state();
        let app = router(state);
        let (status, json) = get(&app, "/processors").await;
        assert_eq!(status, StatusCode::OK);
        let ids = json["processors"].as_array().unwrap();
        assert!(ids.iter().any(|v| v.as_str() == Some("c2pa-verify")));
    }

    // ---- GET /solana-keys ----

    #[tokio::test]
    async fn solana_keys_returns_pubkey() {
        let state = test_state();
        let app = router(state);
        let (status, json) = get(&app, "/solana-keys").await;
        assert_eq!(status, StatusCode::OK);
        let pk = json["solana_pubkey"].as_str().unwrap();
        assert!(pk.len() >= 32 && pk.len() <= 44);
    }

    // ---- POST /process ----

    #[tokio::test]
    async fn process_rejects_unsigned_content() {
        let mut fetcher = MockFetcher::new();
        // Plain JPEG (no C2PA signature)
        let jpeg = create_test_jpeg();
        fetcher.add(
            "https://storage.example.com/photo.jpg",
            jpeg,
            Some("image/jpeg"),
        );
        let state = test_state_with_fetcher(fetcher);
        let app = router(state);

        let body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://storage.example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        let (status, json) = post_json(&app, "/process", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"].as_str().unwrap().contains("signature_hash"));
    }

    #[tokio::test]
    async fn process_signed_content_success() {
        let mut fetcher = MockFetcher::new();
        let signed = create_signed_jpeg();
        fetcher.add(
            "https://storage.example.com/signed.jpg",
            signed,
            Some("image/jpeg"),
        );
        let state = test_state_with_fetcher(fetcher);
        let app = router(state);

        let body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://storage.example.com/signed.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        let (status, json) = post_json(&app, "/process", body).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["signature_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(json["results"]["c2pa-verify"]["status"], "ok");
        assert!(json["attestation"].is_string());
    }

    #[tokio::test]
    async fn process_fetch_failure() {
        let state = test_state();
        let app = router(state);

        let body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://storage.example.com/missing.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        let (status, _json) = post_json(&app, "/process", body).await;
        // Upstream fetch failure surfaces as 502 BAD_GATEWAY — the TEE
        // itself is healthy; the client-supplied URL is not reachable.
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    // ---- POST /extension/solana (parameter validation) ----

    #[tokio::test]
    async fn solana_extension_rejects_bad_pubkey() {
        let state = test_state();
        let app = router(state);

        let body = serde_json::json!({
            "offchain_data_url": "https://example.com/data.json",
            "payer": "not-a-pubkey",
            "merkle_tree": "not-a-pubkey",
            "recent_blockhash": "not-a-hash"
        });

        let (status, json) = post_json(&app, "/extension/solana", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"].as_str().unwrap().contains("invalid"));
    }

    // ---- Helpers ----

    fn create_test_jpeg() -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, Rgb};
        use std::io::Cursor;

        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(4, 4, |x, y| Rgb([(x * 60) as u8, (y * 60) as u8, 128]));

        let mut buf = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut buf)
            .write_image(img.as_raw(), 4, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        buf.into_inner()
    }

    fn create_signed_jpeg() -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer = c2pa::EphemeralSigner::new("tee-server-test")
            .expect("Failed to create EphemeralSigner");

        let definition = serde_json::json!({
            "claim_generator_info": [{
                "name": "tee-server-test",
                "version": "0.1.0"
            }],
            "assertions": [{
                "label": "c2pa.actions.v2",
                "data": {
                    "actions": [{
                        "action": "c2pa.created",
                        "digitalSourceType":
                            "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"
                    }]
                }
            }]
        });

        let mut source = std::io::Cursor::new(&test_jpeg);
        let mut output = std::io::Cursor::new(Vec::new());

        c2pa::Builder::from_context(c2pa::Context::default())
            .with_definition(definition.to_string())
            .expect("Builder definition failed")
            .sign(&signer, "image/jpeg", &mut source, &mut output)
            .expect("Signing failed");

        output.into_inner()
    }
}
