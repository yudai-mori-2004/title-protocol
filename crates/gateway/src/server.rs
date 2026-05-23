// SPDX-License-Identifier: Apache-2.0

//! # Gateway HTTP Server
//!
//! Spec §2.5, §5.3
//!
//! Axum-based HTTP server exposing the Gateway API.
//! Includes API key authentication and rate limiting middleware.

use std::sync::Arc;

use axum::middleware;
use axum::Router;

use crate::auth::{api_key_auth, ApiKeySet};
use crate::endpoints;
use crate::rate_limit::RateLimiter;
use crate::state::{self, GatewayState};
use crate::tee_client::TeeClient;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Gateway server configuration.
pub struct GatewayConfig {
    /// Address to bind (e.g., "0.0.0.0:3000").
    pub bind_addr: String,

    /// TEE client for internal communication.
    pub tee_client: Box<dyn TeeClient>,

    /// API keys for client authentication.
    /// Empty set disables auth (development mode).
    pub api_keys: Vec<String>,

    /// Rate limit: max requests per window.
    pub rate_limit_max: u32,

    /// Rate limit: window duration in seconds.
    pub rate_limit_window_secs: u64,

    /// TEE health check interval in seconds.
    pub health_check_interval_secs: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:3000".to_string(),
            tee_client: Box::new(crate::tee_client::HttpTeeClient::new(
                "http://localhost:4000".to_string(),
            )),
            api_keys: vec![],
            rate_limit_max: 100,
            rate_limit_window_secs: 60,
            health_check_interval_secs: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the Gateway Axum router.
/// Spec §2.5
///
/// Six endpoints with API key auth + rate limiting middleware.
pub fn router(state: Arc<GatewayState>) -> Router {
    Router::new()
        .route("/keys", axum::routing::get(endpoints::handle_keys))
        .route(
            "/processors",
            axum::routing::get(endpoints::handle_processors),
        )
        .route("/process", axum::routing::post(endpoints::handle_process))
        .route("/health", axum::routing::get(endpoints::handle_health))
        .route(
            "/solana-keys",
            axum::routing::get(endpoints::handle_solana_keys),
        )
        .route(
            "/extension/solana",
            axum::routing::post(endpoints::handle_solana_extension),
        )
        .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Server startup
// ---------------------------------------------------------------------------

/// Run the Gateway HTTP server.
/// Spec §2.5, §5.3
///
/// 1. Creates shared state from config
/// 2. Attempts initial TEE info fetch
/// 3. Spawns background health checker
/// 4. Starts the Axum HTTP server
pub async fn run(config: GatewayConfig) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(GatewayState::new(
        config.tee_client,
        ApiKeySet::new(config.api_keys),
        RateLimiter::new(config.rate_limit_max, config.rate_limit_window_secs),
    ));

    // Initial TEE info fetch (non-fatal if TEE isn't up yet)
    if let Err(e) = state.refresh_tee_info().await {
        tracing::warn!(error = %e, "Initial TEE info fetch failed; will retry via health check");
    }

    // Background health checker
    state::spawn_health_check(state.clone(), config.health_check_interval_secs);

    let app = router(state);

    tracing::info!(addr = %config.bind_addr, "Gateway starting");
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tee_client::TeeClientError;
    use crate::{
        HealthResponse, KeysResponse, ProcessorsResponse, SolanaExtensionRequest,
        SolanaExtensionResponse, SolanaKeysResponse,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use title_core::{ProcessRequest, ProcessResponse, VerifiableResponse};

    // ---- Mock TEE Client ----

    pub(crate) struct MockTeeClient {
        pub health_response: Mutex<Option<HealthResponse>>,
        pub keys_response: Mutex<Option<KeysResponse>>,
        pub processors_response: Mutex<Option<ProcessorsResponse>>,
        pub process_response: Mutex<Option<ProcessResponse>>,
        pub solana_keys_response: Mutex<Option<SolanaKeysResponse>>,
        pub solana_ext_response: Mutex<Option<SolanaExtensionResponse>>,
        pub should_fail: Mutex<bool>,
    }

    impl MockTeeClient {
        pub(crate) fn new() -> Self {
            Self {
                health_response: Mutex::new(Some(HealthResponse {
                    status: "ok".into(),
                    tee_type: Some("mock".into()),
                })),
                keys_response: Mutex::new(Some(KeysResponse {
                    keys: {
                        let mut m = HashMap::new();
                        m.insert("x25519".into(), "mock-key-x25519".into());
                        m
                    },
                })),
                processors_response: Mutex::new(Some(ProcessorsResponse {
                    processors: vec!["c2pa-verify".into()],
                })),
                process_response: Mutex::new(Some(ProcessResponse {
                    verifiable: VerifiableResponse {
                        signature_hash: "sha256:mock".into(),
                        results: HashMap::new(),
                    },
                    attestation: "mock-attestation".into(),
                })),
                solana_keys_response: Mutex::new(None),
                solana_ext_response: Mutex::new(None),
                should_fail: Mutex::new(false),
            }
        }

        pub(crate) fn with_solana(self) -> Self {
            *self.solana_keys_response.lock().unwrap() = Some(SolanaKeysResponse {
                solana_pubkey: "MockSolanaPubkey".into(),
            });
            *self.solana_ext_response.lock().unwrap() = Some(SolanaExtensionResponse {
                partial_tx: "mock-partial-tx".into(),
            });
            self
        }

        pub(crate) fn failing(self) -> Self {
            *self.should_fail.lock().unwrap() = true;
            self
        }
    }

    #[async_trait]
    impl TeeClient for MockTeeClient {
        async fn health(&self) -> Result<HealthResponse, TeeClientError> {
            if *self.should_fail.lock().unwrap() {
                return Err(TeeClientError::Unreachable("mock failure".into()));
            }
            self.health_response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| TeeClientError::Unreachable("no mock".into()))
        }

        async fn keys(&self) -> Result<KeysResponse, TeeClientError> {
            if *self.should_fail.lock().unwrap() {
                return Err(TeeClientError::Unreachable("mock failure".into()));
            }
            self.keys_response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| TeeClientError::Unreachable("no mock".into()))
        }

        async fn processors(&self) -> Result<ProcessorsResponse, TeeClientError> {
            if *self.should_fail.lock().unwrap() {
                return Err(TeeClientError::Unreachable("mock failure".into()));
            }
            self.processors_response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| TeeClientError::Unreachable("no mock".into()))
        }

        async fn process(
            &self,
            _req: &ProcessRequest,
        ) -> Result<ProcessResponse, TeeClientError> {
            if *self.should_fail.lock().unwrap() {
                return Err(TeeClientError::Unreachable("mock failure".into()));
            }
            self.process_response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| TeeClientError::Unreachable("no mock".into()))
        }

        async fn solana_keys(&self) -> Result<Option<SolanaKeysResponse>, TeeClientError> {
            if *self.should_fail.lock().unwrap() {
                return Err(TeeClientError::Unreachable("mock failure".into()));
            }
            Ok(self.solana_keys_response.lock().unwrap().clone())
        }

        async fn solana_extension(
            &self,
            _req: &SolanaExtensionRequest,
        ) -> Result<SolanaExtensionResponse, TeeClientError> {
            if *self.should_fail.lock().unwrap() {
                return Err(TeeClientError::Unreachable("mock failure".into()));
            }
            self.solana_ext_response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| TeeClientError::Unreachable("no mock".into()))
        }
    }

    /// Create a test state with mock TEE client and no auth.
    pub(crate) fn test_state(client: MockTeeClient) -> Arc<GatewayState> {
        Arc::new(GatewayState::new(
            Box::new(client),
            ApiKeySet::empty(),
            RateLimiter::new(1000, 60),
        ))
    }

    /// Create a test state with API key auth.
    pub(crate) fn test_state_with_auth(
        client: MockTeeClient,
        keys: Vec<String>,
    ) -> Arc<GatewayState> {
        Arc::new(GatewayState::new(
            Box::new(client),
            ApiKeySet::new(keys),
            RateLimiter::new(1000, 60),
        ))
    }

    // ---- Helper to make HTTP requests against the router ----

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn send_get(app: &Router, path: &str) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        (status, json)
    }

    async fn send_get_with_auth(
        app: &Router,
        path: &str,
        api_key: &str,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .uri(path)
            .header("authorization", format!("Bearer {api_key}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        (status, json)
    }

    async fn send_post(
        app: &Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        (status, json)
    }

    async fn send_post_with_auth(
        app: &Router,
        path: &str,
        body: serde_json::Value,
        api_key: &str,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {api_key}"))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        (status, json)
    }

    // ---- GET /health ----

    #[tokio::test]
    async fn health_returns_ok_when_tee_available() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, body) = send_get(&app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["tee_type"], "mock");
    }

    #[tokio::test]
    async fn health_returns_unavailable_when_tee_down() {
        let state = test_state(MockTeeClient::new().failing());
        let app = router(state);

        let (status, body) = send_get(&app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "unavailable");
    }

    // ---- GET /keys ----

    #[tokio::test]
    async fn keys_returns_cached_keys() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, body) = send_get(&app, "/keys").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["keys"]["x25519"], "mock-key-x25519");
    }

    #[tokio::test]
    async fn keys_returns_503_when_not_cached() {
        let state = test_state(MockTeeClient::new());
        // Don't refresh -- cache is empty
        let app = router(state);

        let (status, _body) = send_get(&app, "/keys").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    // ---- GET /processors ----

    #[tokio::test]
    async fn processors_returns_list() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, body) = send_get(&app, "/processors").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["processors"][0], "c2pa-verify");
    }

    // ---- POST /process ----

    #[tokio::test]
    async fn process_relays_to_tee() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let req_body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        let (status, body) = send_post(&app, "/process", req_body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["signature_hash"], "sha256:mock");
        assert_eq!(body["attestation"], "mock-attestation");
    }

    #[tokio::test]
    async fn process_returns_503_when_tee_unavailable() {
        let state = test_state(MockTeeClient::new());
        // Don't refresh -- TEE is not available
        let app = router(state);

        let req_body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        let (status, _body) = send_post(&app, "/process", req_body).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn process_returns_503_when_tee_unreachable() {
        // Mock where health works but process is None → Unreachable error
        let mock = MockTeeClient::new();
        *mock.process_response.lock().unwrap() = None;
        let state = test_state(mock);
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let req_body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        let (status, _) = send_post(&app, "/process", req_body).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    // ---- GET /solana-keys ----

    #[tokio::test]
    async fn solana_keys_returns_pubkey_when_enabled() {
        let state = test_state(MockTeeClient::new().with_solana());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, body) = send_get(&app, "/solana-keys").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["solana_pubkey"], "MockSolanaPubkey");
    }

    #[tokio::test]
    async fn solana_keys_returns_404_when_disabled() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, _body) = send_get(&app, "/solana-keys").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // ---- POST /extension/solana ----

    #[tokio::test]
    async fn solana_extension_relays_to_tee() {
        let state = test_state(MockTeeClient::new().with_solana());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let req_body = serde_json::json!({
            "offchain_data_url": "https://example.com/data.json",
            "payer": "Base58Payer",
            "merkle_tree": "Base58Tree",
            "recent_blockhash": "Base58Hash",
            "collection": "Base58Col"
        });

        let (status, body) = send_post(&app, "/extension/solana", req_body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["partial_tx"], "mock-partial-tx");
    }

    #[tokio::test]
    async fn solana_extension_returns_404_when_disabled() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let req_body = serde_json::json!({
            "offchain_data_url": "https://example.com/data.json",
            "payer": "Base58Payer",
            "merkle_tree": "Base58Tree",
            "recent_blockhash": "Base58Hash"
        });

        let (status, _) = send_post(&app, "/extension/solana", req_body).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // ---- Authentication ----

    #[tokio::test]
    async fn auth_rejects_without_key() {
        let state = test_state_with_auth(
            MockTeeClient::new(),
            vec!["valid-key".into()],
        );
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, _) = send_get(&app, "/keys").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_rejects_invalid_key() {
        let state = test_state_with_auth(
            MockTeeClient::new(),
            vec!["valid-key".into()],
        );
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, _) = send_get_with_auth(&app, "/keys", "wrong-key").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_accepts_valid_key() {
        let state = test_state_with_auth(
            MockTeeClient::new(),
            vec!["valid-key".into()],
        );
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (status, body) = send_get_with_auth(&app, "/keys", "valid-key").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["keys"].is_object());
    }

    #[tokio::test]
    async fn health_skips_auth() {
        let state = test_state_with_auth(
            MockTeeClient::new(),
            vec!["valid-key".into()],
        );
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        // Health should work without auth
        let (status, body) = send_get(&app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn auth_disabled_when_no_keys_configured() {
        let state = test_state(MockTeeClient::new());
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        // No auth configured, all requests should pass
        let (status, _) = send_get(&app, "/keys").await;
        assert_eq!(status, StatusCode::OK);
    }

    // ---- Rate limiting ----

    #[tokio::test]
    async fn rate_limit_rejects_excess_requests() {
        let state = Arc::new(GatewayState::new(
            Box::new(MockTeeClient::new()),
            ApiKeySet::new(vec!["key1".into()]),
            RateLimiter::new(2, 60), // Only 2 requests per minute
        ));
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let (s1, _) = send_get_with_auth(&app, "/keys", "key1").await;
        let (s2, _) = send_get_with_auth(&app, "/keys", "key1").await;
        let (s3, _) = send_get_with_auth(&app, "/keys", "key1").await;
        assert_eq!(s1, StatusCode::OK);
        assert_eq!(s2, StatusCode::OK);
        assert_eq!(s3, StatusCode::TOO_MANY_REQUESTS);
    }

    // ---- TEE restart detection ----

    #[tokio::test]
    async fn refresh_updates_cache() {
        let state = test_state(MockTeeClient::new());

        // Initially cache is empty
        assert!(!state.is_tee_available());
        {
            let cache = state.tee_cache.read().await;
            assert!(cache.keys.is_none());
        }

        state.refresh_tee_info().await.unwrap();

        assert!(state.is_tee_available());
        {
            let cache = state.tee_cache.read().await;
            assert!(cache.keys.is_some());
            assert!(cache.processors.is_some());
        }
    }

    #[tokio::test]
    async fn check_and_refresh_detects_tee_down() {
        // Create a mock that will fail health checks
        let mock = MockTeeClient::new().failing();
        let state = test_state(mock);

        // Force tee_available to true to simulate previous good state
        state
            .tee_available
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(state.is_tee_available());

        // check_and_refresh should detect failure and mark unavailable
        state.check_and_refresh().await;
        assert!(!state.is_tee_available());
    }

    // ---- POST /process with auth ----

    #[tokio::test]
    async fn process_with_auth() {
        let state = test_state_with_auth(
            MockTeeClient::new(),
            vec!["my-api-key".into()],
        );
        state.refresh_tee_info().await.unwrap();
        let app = router(state);

        let req_body = serde_json::json!({
            "input_type": "single",
            "content_url": "https://example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        });

        // Without auth: 401
        let (status, _) = send_post(&app, "/process", req_body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // With auth: 200
        let (status, body) =
            send_post_with_auth(&app, "/process", req_body, "my-api-key").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["signature_hash"], "sha256:mock");
    }
}
