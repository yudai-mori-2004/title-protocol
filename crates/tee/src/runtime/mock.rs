// SPDX-License-Identifier: Apache-2.0

//! # Mock TEE Runtime
//!
//! Spec §5.2
//!
//! Development/testing runtime that runs outside any TEE hardware.
//! Uses OS-provided CSPRNG for random bytes and produces a deterministic
//! mock attestation format: `"mock-attestation:" + user_data`.

use rand::RngCore;

use crate::{TeeError, TeeRuntime};

/// Mock TEE runtime for local development and testing.
/// Spec §5.2
///
/// - `random_bytes`: delegates to `OsRng` (OS CSPRNG).
/// - `get_attestation_document`: returns `"mock-attestation:" + user_data`.
///   This is NOT cryptographically meaningful — it exists solely so that
///   downstream code can exercise the attestation flow without TEE hardware.
pub struct MockRuntime;

impl MockRuntime {
    // `.default()` 呼出ゼロの dead surface を再追加しないため
    // `clippy::new_without_default` を抑制する。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self
    }
}

impl TeeRuntime for MockRuntime {
    fn tee_type(&self) -> &str {
        "mock"
    }

    /// Spec §1.2, §5.2 — mock attestation document.
    ///
    /// Format: `b"mock-attestation:" ++ user_data`
    ///
    /// In production (NitroRuntime), this returns a COSE_Sign1 document
    /// signed by the hypervisor's key and containing PCR measurements.
    fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError> {
        let mut doc = b"mock-attestation:".to_vec();
        doc.extend_from_slice(user_data);
        Ok(doc)
    }

    /// Spec §5.2 — cryptographically secure random bytes via OsRng.
    fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError> {
        let mut buf = vec![0u8; len];
        rand::rngs::OsRng
            .try_fill_bytes(&mut buf)
            .map_err(|e| TeeError::RandomFailed(e.to_string()))?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tee_type_is_mock() {
        let rt = MockRuntime::new();
        assert_eq!(rt.tee_type(), "mock");
    }

    #[test]
    fn attestation_document_format() {
        let rt = MockRuntime::new();
        let user_data = b"sha256:abcdef";
        let doc = rt.get_attestation_document(user_data).unwrap();
        assert!(doc.starts_with(b"mock-attestation:"));
        assert_eq!(&doc[b"mock-attestation:".len()..], user_data);
    }

    #[test]
    fn attestation_document_empty_user_data() {
        let rt = MockRuntime::new();
        let doc = rt.get_attestation_document(b"").unwrap();
        assert_eq!(doc, b"mock-attestation:");
    }

    #[test]
    fn random_bytes_length() {
        let rt = MockRuntime::new();
        for len in [0, 1, 32, 64, 256] {
            let bytes = rt.random_bytes(len).unwrap();
            assert_eq!(bytes.len(), len);
        }
    }

    #[test]
    fn random_bytes_not_all_zero() {
        let rt = MockRuntime::new();
        let bytes = rt.random_bytes(32).unwrap();
        // Probability of 32 zero bytes from CSPRNG is astronomically low
        assert!(bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn random_bytes_distinct() {
        let rt = MockRuntime::new();
        let a = rt.random_bytes(32).unwrap();
        let b = rt.random_bytes(32).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn trait_object_safety() {
        let rt: Box<dyn TeeRuntime> = Box::new(MockRuntime::new());
        assert_eq!(rt.tee_type(), "mock");
        let doc = rt.get_attestation_document(b"test").unwrap();
        assert!(doc.starts_with(b"mock-attestation:"));
        let rand = rt.random_bytes(16).unwrap();
        assert_eq!(rand.len(), 16);
    }
}
