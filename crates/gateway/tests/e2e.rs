// SPDX-License-Identifier: Apache-2.0

//! # Gateway ↔ TEE End-to-End Integration Tests
//!
//! Spec §2.1, §5.3
//!
//! These tests start real TEE and Gateway HTTP servers in the same
//! process and exercise the full Client → Gateway → TEE flow.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::net::TcpListener;

use title_core::{C2paVerifyProcessor, ProcessorRegistry};
use title_crypto::key_bundle::KeyBundle;
use title_gateway::server::{self, GatewayConfig};
use title_gateway::tee_client::HttpTeeClient;
use title_solana::signing_key::SolanaSigningKey;
use title_tee::content_fetch::{ContentFetcher, FetchError, FetchResponse};
use title_tee::resource_pool::ResourcePool;
use title_tee::runtime::mock::MockRuntime;
use title_tee::server::{self as tee_server, TeeAppState};

// ---------------------------------------------------------------------------
// Test content fetcher
// ---------------------------------------------------------------------------

struct TestFetcher {
    responses: HashMap<String, (Vec<u8>, Option<String>)>,
}

impl TestFetcher {
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

impl ContentFetcher for TestFetcher {
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

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn build_tee_state(fetcher: TestFetcher) -> Arc<TeeAppState> {
    let mut registry = ProcessorRegistry::new();
    registry.register(Box::new(C2paVerifyProcessor::new()));

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

async fn start_tee(state: Arc<TeeAppState>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = tee_server::router(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn start_gateway(tee_endpoint: &str, api_keys: Vec<String>) -> String {
    let tee_client = HttpTeeClient::new(tee_endpoint.to_string());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let config = GatewayConfig {
        bind_addr: addr.to_string(),
        tee_client: Box::new(tee_client),
        api_keys,
        rate_limit_max: 1000,
        rate_limit_window_secs: 60,
        health_check_interval_secs: 60,
    };

    let state = Arc::new(title_gateway::state::GatewayState::new(
        config.tee_client,
        title_gateway::auth::ApiKeySet::new(config.api_keys),
        title_gateway::rate_limit::RateLimiter::new(
            config.rate_limit_max,
            config.rate_limit_window_secs,
        ),
    ));

    state.refresh_tee_info().await.unwrap();

    title_gateway::state::spawn_health_check(state.clone(), config.health_check_interval_secs);

    let app = server::router(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}")
}

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
    let signer = c2pa::EphemeralSigner::new("e2e-test").expect("Failed to create EphemeralSigner");

    let definition = serde_json::json!({
        "claim_generator_info": [{"name": "e2e-test", "version": "0.1.0"}],
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

// ---------------------------------------------------------------------------
// E2E Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn e2e_health_through_gateway() {
    let tee_url = start_tee(build_tee_state(TestFetcher::new())).await;
    let gw_url = start_gateway(&tee_url, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{gw_url}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["tee_type"], "mock");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_keys_through_gateway() {
    let tee_url = start_tee(build_tee_state(TestFetcher::new())).await;
    let gw_url = start_gateway(&tee_url, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{gw_url}/keys"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let keys = body["keys"].as_object().unwrap();
    assert!(keys.contains_key("x25519"));
    assert!(keys.contains_key("p256"));
    assert!(keys.contains_key("ml-kem-768"));
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_processors_through_gateway() {
    let tee_url = start_tee(build_tee_state(TestFetcher::new())).await;
    let gw_url = start_gateway(&tee_url, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{gw_url}/processors"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let processors = body["processors"].as_array().unwrap();
    assert!(processors.iter().any(|v| v.as_str() == Some("c2pa-verify")));
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_solana_keys_through_gateway() {
    let tee_url = start_tee(build_tee_state(TestFetcher::new())).await;
    let gw_url = start_gateway(&tee_url, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{gw_url}/solana-keys"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["solana_pubkey"].as_str().unwrap().len() >= 32);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_process_signed_content() {
    let mut fetcher = TestFetcher::new();
    let signed_jpeg = create_signed_jpeg();
    fetcher.add(
        "https://storage.example.com/signed.jpg",
        signed_jpeg,
        Some("image/jpeg"),
    );

    let tee_url = start_tee(build_tee_state(fetcher)).await;
    let gw_url = start_gateway(&tee_url, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{gw_url}/process"))
        .json(&serde_json::json!({
            "input_type": "single",
            "content_url": "https://storage.example.com/signed.jpg",
            "processor_ids": ["c2pa-verify"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["signature_hash"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(body["results"]["c2pa-verify"]["status"], "ok");
    assert!(body["attestation"].is_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_process_unsigned_content_returns_error() {
    let mut fetcher = TestFetcher::new();
    fetcher.add(
        "https://storage.example.com/photo.jpg",
        create_test_jpeg(),
        Some("image/jpeg"),
    );

    let tee_url = start_tee(build_tee_state(fetcher)).await;
    let gw_url = start_gateway(&tee_url, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{gw_url}/process"))
        .json(&serde_json::json!({
            "input_type": "single",
            "content_url": "https://storage.example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        }))
        .send()
        .await
        .unwrap();

    assert_ne!(resp.status(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_api_key_auth() {
    let tee_url = start_tee(build_tee_state(TestFetcher::new())).await;
    let gw_url = start_gateway(&tee_url, vec!["secret-key".into()]).await;

    let client = reqwest::Client::new();

    // Without auth: 401
    let resp = client
        .get(format!("{gw_url}/keys"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // With wrong key: 401
    let resp = client
        .get(format!("{gw_url}/keys"))
        .header("authorization", "Bearer wrong-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // With correct key: 200
    let resp = client
        .get(format!("{gw_url}/keys"))
        .header("authorization", "Bearer secret-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Health always works without auth
    let resp = client
        .get(format!("{gw_url}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_tee_restart_detection() {
    // Start TEE #1
    let tee_state_1 = build_tee_state(TestFetcher::new());
    let key1 = {
        let keys = tee_state_1.key_bundle.public_keys();
        keys["x25519"].clone()
    };
    let tee_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tee_addr = tee_listener.local_addr().unwrap();
    let tee_url = format!("http://{tee_addr}");
    let tee_app_1 = tee_server::router(tee_state_1);
    let tee_handle = tokio::spawn(async move {
        axum::serve(tee_listener, tee_app_1).await.unwrap();
    });

    // Start Gateway with short health check interval
    let tee_client = HttpTeeClient::new(tee_url.clone());
    let gw_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gw_addr = gw_listener.local_addr().unwrap();
    let gw_url = format!("http://{gw_addr}");

    let gw_state = Arc::new(title_gateway::state::GatewayState::new(
        Box::new(tee_client),
        title_gateway::auth::ApiKeySet::empty(),
        title_gateway::rate_limit::RateLimiter::new(1000, 60),
    ));
    gw_state.refresh_tee_info().await.unwrap();
    title_gateway::state::spawn_health_check(gw_state.clone(), 1);

    let app = server::router(gw_state);
    tokio::spawn(async move {
        axum::serve(gw_listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Verify initial keys through Gateway
    let resp = client
        .get(format!("{gw_url}/keys"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["keys"]["x25519"], key1);

    // Stop TEE #1
    tee_handle.abort();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Start TEE #2 on the same port with different keys
    let tee_state_2 = build_tee_state(TestFetcher::new());
    let key2 = {
        let keys = tee_state_2.key_bundle.public_keys();
        keys["x25519"].clone()
    };
    assert_ne!(key1, key2);

    let tee_listener_2 = TcpListener::bind(tee_addr).await.unwrap();
    let tee_app_2 = tee_server::router(tee_state_2);
    tokio::spawn(async move {
        axum::serve(tee_listener_2, tee_app_2).await.unwrap();
    });

    // Wait for Gateway's health check to detect restart
    // (TEE was unavailable → becomes available → triggers refresh)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Verify Gateway cache was refreshed with TEE #2's keys
    let resp = client
        .get(format!("{gw_url}/keys"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["keys"]["x25519"], key2);
}
