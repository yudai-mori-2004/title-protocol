// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol TEE Runtime
//!
//! Spec §5.2 — hardware abstraction for the TEE (Attestation Document
//! retrieval, entropy). Vendor backends are selected at compile time via
//! the `vendor-aws` / `runtime-mock` features.

pub mod content_fetch;
pub mod limits;
pub mod orchestrator;
pub mod proxy_fetcher;
pub mod resource_pool;
pub mod runtime;
pub mod server;
pub mod vendor;

/// TEE runtime error type.
#[derive(Debug, thiserror::Error)]
pub enum TeeError {
    #[error("Failed to get attestation document: {0}")]
    AttestationFailed(String),

    #[error("Failed to generate random bytes: {0}")]
    RandomFailed(String),

    #[error("TEE initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Vendor-specific error: {0}")]
    VendorError(String),
}

/// TEE hardware abstraction. Spec §5.2.
///
/// Implementations are stateless across requests; secret keys live only in
/// TEE memory (§0.5, §5.2).
pub trait TeeRuntime: Send + Sync {
    /// Vendor tag — e.g. `"aws-nitro"`, `"mock"`.
    fn tee_type(&self) -> &str;

    /// Capture an Attestation Document with `user_data` bound into it
    /// (§1.2, §5.2). The returned bytes are the vendor-specific document
    /// format (COSE_Sign1 CBOR on AWS Nitro).
    fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError>;

    /// Cryptographically-secure entropy sourced inside the TEE.
    fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubRuntime;

    impl TeeRuntime for StubRuntime {
        fn tee_type(&self) -> &str {
            "mock"
        }
        fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError> {
            Ok(user_data.to_vec())
        }
        fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError> {
            Ok(vec![0u8; len])
        }
    }

    #[test]
    fn trait_object_safety() {
        let rt: Box<dyn TeeRuntime> = Box::new(StubRuntime);
        assert_eq!(rt.tee_type(), "mock");
    }

    #[test]
    fn stub_attestation_round_trips_user_data() {
        let rt = StubRuntime;
        let user_data = b"sha256:test_hash";
        assert_eq!(rt.get_attestation_document(user_data).unwrap(), user_data);
    }

    #[test]
    fn stub_random_bytes_returns_requested_length() {
        let rt = StubRuntime;
        assert_eq!(rt.random_bytes(32).unwrap().len(), 32);
    }
}
