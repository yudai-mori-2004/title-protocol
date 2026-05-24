// SPDX-License-Identifier: Apache-2.0

//! # TEE Client
//!
//! Spec §5.3
//!
//! Abstraction over the Gateway-to-TEE communication channel.
//! The HttpTeeClient calls TEE's internal HTTP API.
//! Mock implementations are used in tests.

use async_trait::async_trait;

use title_core::{ProcessRequest, ProcessResponse};

use crate::{
    HealthResponse, KeysResponse, ProcessorsResponse, SolanaExtensionRequest,
    SolanaExtensionResponse, SolanaKeysResponse,
};

/// Outcome of relaying `POST /process` to the TEE. The Gateway is a thin
/// pass-through (spec §2.5, §1.7) — encrypted requests come back as raw
/// `nonce || ciphertext` bytes (spec §2.4), so we cannot assume JSON.
#[derive(Debug)]
pub enum ProcessOutcome {
    /// Plaintext JSON `ProcessResponse` body (unencrypted request path).
    Plaintext(ProcessResponse),
    /// Sealed bytes (`application/octet-stream`) returned by the TEE for an
    /// encrypted request — passed through to the client verbatim.
    Encrypted(Vec<u8>),
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error from TEE communication.
#[derive(Debug, thiserror::Error)]
pub enum TeeClientError {
    /// Network-level failure (connection refused, DNS, timeout).
    #[error("TEE unreachable: {0}")]
    Unreachable(String),

    /// TEE returned a non-success HTTP status.
    #[error("TEE returned HTTP {status}: {body}")]
    HttpError { status: u16, body: String },

    /// Response deserialization failed.
    #[error("TEE response parse error: {0}")]
    ParseError(String),
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// TEE client interface.
/// Spec §5.3 -- Gateway communicates with TEE over internal HTTP.
#[async_trait]
pub trait TeeClient: Send + Sync {
    /// GET /health on TEE.
    async fn health(&self) -> Result<HealthResponse, TeeClientError>;

    /// GET /keys on TEE.
    async fn keys(&self) -> Result<KeysResponse, TeeClientError>;

    /// GET /processors on TEE.
    async fn processors(&self) -> Result<ProcessorsResponse, TeeClientError>;

    /// POST /process on TEE. Returns either a plaintext `ProcessResponse`
    /// or, for encrypted requests, the raw sealed bytes the TEE produced.
    async fn process(&self, req: &ProcessRequest) -> Result<ProcessOutcome, TeeClientError>;

    /// GET /solana-keys on TEE. Returns None if extension not enabled.
    async fn solana_keys(&self) -> Result<Option<SolanaKeysResponse>, TeeClientError>;

    /// POST /extension/solana on TEE.
    async fn solana_extension(
        &self,
        req: &SolanaExtensionRequest,
    ) -> Result<SolanaExtensionResponse, TeeClientError>;
}

// ---------------------------------------------------------------------------
// HTTP implementation
// ---------------------------------------------------------------------------

/// HTTP-based TEE client.
/// Spec §5.3
///
/// Calls TEE's internal HTTP endpoints. The TEE endpoint is typically
/// on the same machine or network (e.g., vsock for Nitro Enclaves,
/// localhost for development).
pub struct HttpTeeClient {
    endpoint: String,
    client: reqwest::Client,
}

impl HttpTeeClient {
    pub fn new(endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("title-gateway/0.1.2")
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(5))
            .pool_max_idle_per_host(16)
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");
        Self { endpoint, client }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, TeeClientError> {
        let url = format!("{}{}", self.endpoint, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| TeeClientError::Unreachable(e.to_string()))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = read_error_body(resp).await;
            return Err(TeeClientError::HttpError { status, body });
        }

        resp.json()
            .await
            .map_err(|e| TeeClientError::ParseError(e.to_string()))
    }

    async fn post<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &Req,
    ) -> Result<Resp, TeeClientError> {
        let url = format!("{}{}", self.endpoint, path);
        let resp = self
            .client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| TeeClientError::Unreachable(e.to_string()))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = read_error_body(resp).await;
            return Err(TeeClientError::HttpError { status, body });
        }

        resp.json()
            .await
            .map_err(|e| TeeClientError::ParseError(e.to_string()))
    }
}

/// Read an error response body as a String, recording the read failure
/// inline instead of returning an empty body that would silently look
/// like "TEE answered with no message".
async fn read_error_body(resp: reqwest::Response) -> String {
    match resp.text().await {
        Ok(s) => s,
        Err(e) => format!("<body read failed: {e}>"),
    }
}

#[async_trait]
impl TeeClient for HttpTeeClient {
    async fn health(&self) -> Result<HealthResponse, TeeClientError> {
        self.get("/health").await
    }

    async fn keys(&self) -> Result<KeysResponse, TeeClientError> {
        self.get("/keys").await
    }

    async fn processors(&self) -> Result<ProcessorsResponse, TeeClientError> {
        self.get("/processors").await
    }

    async fn process(&self, req: &ProcessRequest) -> Result<ProcessOutcome, TeeClientError> {
        let url = format!("{}/process", self.endpoint);
        let resp = self
            .client
            .post(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| TeeClientError::Unreachable(e.to_string()))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = read_error_body(resp).await;
            return Err(TeeClientError::HttpError { status, body });
        }

        // §2.4: encrypted responses are `application/octet-stream`; pass them
        // through verbatim instead of forcing JSON.
        let is_octet_stream = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.starts_with("application/octet-stream"))
            .unwrap_or(false);

        if is_octet_stream {
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| TeeClientError::ParseError(e.to_string()))?
                .to_vec();
            Ok(ProcessOutcome::Encrypted(bytes))
        } else {
            let body: ProcessResponse = resp
                .json()
                .await
                .map_err(|e| TeeClientError::ParseError(e.to_string()))?;
            Ok(ProcessOutcome::Plaintext(body))
        }
    }

    async fn solana_keys(&self) -> Result<Option<SolanaKeysResponse>, TeeClientError> {
        match self.get::<SolanaKeysResponse>("/solana-keys").await {
            Ok(resp) => Ok(Some(resp)),
            Err(TeeClientError::HttpError { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn solana_extension(
        &self,
        req: &SolanaExtensionRequest,
    ) -> Result<SolanaExtensionResponse, TeeClientError> {
        self.post("/extension/solana", req).await
    }
}
