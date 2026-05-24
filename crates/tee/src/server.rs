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
    /// HTTP content fetcher for external storage.
    pub fetcher: Box<dyn ContentFetcher>,
    /// Server start time for uptime calculation.
    pub started_at: Instant,
}

/// Build the TEE Axum router.
/// Spec §2.5
pub fn router(state: Arc<TeeAppState>) -> Router {
    Router::new()
        .route("/health", get(handle_health))
        .route("/keys", get(handle_keys))
        .route("/processors", get(handle_processors))
        .route("/process", post(handle_process))
        .route("/solana-keys", get(handle_solana_keys))
        .route("/extension/solana", post(handle_solana_extension))
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
async fn handle_process(
    State(state): State<Arc<TeeAppState>>,
    Json(request): Json<ProcessRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let result = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || {
            orchestrator::process_request(
                &request,
                state.fetcher.as_ref(),
                &state.registry,
                state.runtime.as_ref(),
                &state.pool,
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
        Ok(response) => Ok(Json(response)),
        Err(orchestrator::OrchestratorError::AdmissionRejected) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Service busy, try again later" })),
        )),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

/// GET /solana-keys — Solana Extension public key.
/// Spec §2.5, §6.2
async fn handle_solana_keys(State(state): State<Arc<TeeAppState>>) -> impl IntoResponse {
    Json(serde_json::json!({
        "solana_pubkey": state.solana_key.pubkey_base58(),
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

    // Fetch offchain data
    let offchain_resp = state
        .fetcher
        .fetch(&body.offchain_data_url)
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Offchain data fetch failed: {e}") })),
            )
        })?;

    let offchain_data: title_core::ProcessResponse =
        serde_json::from_slice(&offchain_resp.body).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid offchain data: {e}") })),
            )
        })?;

    // Process extension (verify attestation + build & sign TX)
    let tx_bytes =
        extension::process_extension(&state.solana_key, &offchain_data, &ext_request, None)
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
            fetcher: Box::new(fetcher),
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
        assert!(json["signature_hash"].as_str().unwrap().starts_with("sha256:"));
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
        assert_eq!(status, StatusCode::BAD_REQUEST);
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

        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |x, y| {
            Rgb([(x * 60) as u8, (y * 60) as u8, 128])
        });

        let mut buf = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut buf)
            .write_image(img.as_raw(), 4, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        buf.into_inner()
    }

    fn create_signed_jpeg() -> Vec<u8> {
        let test_jpeg = create_test_jpeg();
        let signer =
            c2pa::EphemeralSigner::new("tee-server-test").expect("Failed to create EphemeralSigner");

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
            .with_definition(&definition.to_string())
            .expect("Builder definition failed")
            .sign(&signer, "image/jpeg", &mut source, &mut output)
            .expect("Signing failed");

        output.into_inner()
    }
}
