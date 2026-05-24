// SPDX-License-Identifier: Apache-2.0

//! # Title Attestation
//!
//! Spec §1.2, §5.2, §6.2
//!
//! Vendor-agnostic interface for verifying TEE Attestation Documents.
//! Concrete implementations live in sibling crates (e.g. `title-attestation-aws-nitro`).
//!
//! ## Responsibilities
//!
//! - Parse the vendor-specific Attestation Document format
//! - Verify the signature chain back to the vendor's root CA
//! - Extract measurement (PCR0 etc.), user_data, and public_key fields
//! - Surface a uniform `VerifiedAttestation` regardless of vendor

/// Successfully verified Attestation Document.
///
/// The shape is deliberately vendor-neutral: a primary measurement (variable
/// length, since AWS Nitro uses 48-byte SHA-384, other vendors may differ),
/// a vendor-defined instance identifier, and the two attachable data fields
/// (`user_data` / `public_key`) that every TEE platform supports.
#[derive(Debug, Clone)]
pub struct VerifiedAttestation {
    /// Vendor identifier, e.g. `"aws-nitro"`. Stable across vendor releases.
    pub vendor: &'static str,
    /// Vendor-defined identifier for the TEE instance that produced this
    /// document (e.g. AWS Nitro's `module_id`). Empty if the vendor does not
    /// expose such a field.
    pub instance_id: String,
    /// Document generation time in Unix milliseconds.
    pub timestamp_ms: u64,
    /// Primary measurement of the running TEE image. Length is vendor-
    /// specific: AWS Nitro emits PCR0 as 48-byte SHA-384; AMD SEV-SNP, Intel
    /// TDX, etc. will have their own conventions. Compare for equality only
    /// — no semantic interpretation lives in this crate.
    pub measurement: Vec<u8>,
    /// Arbitrary data the TEE bound into this attestation.
    /// Spec §1.2 — Title Protocol places `JCS(signature_hash + results)` SHA-256 here.
    pub user_data: Option<Vec<u8>>,
    /// Optional public key bound into this attestation (used by Solana Extension
    /// to bind a signing key to the TEE).
    pub public_key: Option<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("attestation parse failed: {0}")]
    ParseFailed(String),

    #[error("certificate chain verification failed: {0}")]
    CertChainInvalid(String),

    #[error("document signature verification failed: {0}")]
    SignatureInvalid(String),

    #[error("required field missing: {0}")]
    MissingField(String),
}

/// Vendor-specific Attestation Document verifier.
///
/// Implementations must perform the full chain of trust verification (signature,
/// certificate path, validity period) before returning `Ok`. Callers may rely on
/// every returned `VerifiedAttestation` being cryptographically authenticated.
pub trait AttestationVerifier {
    /// Vendor tag — must match the `vendor` field on returned `VerifiedAttestation`s.
    fn vendor(&self) -> &'static str;

    /// Parse and fully verify the document.
    ///
    /// `now_unix_secs` is the reference time used for certificate validity
    /// checks. Implementations should reject documents whose internal timestamp
    /// is in the future relative to `now_unix_secs`, and certificates that
    /// expire before `now_unix_secs`.
    fn verify(
        &self,
        doc_bytes: &[u8],
        now_unix_secs: u64,
    ) -> Result<VerifiedAttestation, AttestationError>;
}

// ---------------------------------------------------------------------------
// Mock verifier (feature `mock`)
// ---------------------------------------------------------------------------

#[cfg(feature = "mock")]
mod mock {
    use super::*;

    /// In-memory `AttestationVerifier` for tests.
    ///
    /// Accepts attestations of the form `"mock-attestation:" || user_data`,
    /// returns a zero-measurement `VerifiedAttestation` whose `user_data`
    /// is the trailing bytes. Gated behind the `mock` feature.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct MockAttestationVerifier;

    impl MockAttestationVerifier {
        pub const VENDOR: &'static str = "mock";
        pub const PREFIX: &'static [u8] = b"mock-attestation:";
        /// Measurement reported by the mock — distinctive ASCII banner so
        /// it never collides with a debug-mode AWS Nitro PCR0 (all zeros).
        /// 48 bytes to match the PCR0 wire size. An admin who pastes this
        /// into `add_approved_measurement` is obviously approving the mock.
        pub const MEASUREMENT: [u8; 48] = *b"TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!";

        pub fn new() -> Self {
            Self
        }
    }

    impl AttestationVerifier for MockAttestationVerifier {
        fn vendor(&self) -> &'static str {
            Self::VENDOR
        }

        fn verify(
            &self,
            doc_bytes: &[u8],
            _now_unix_secs: u64,
        ) -> Result<VerifiedAttestation, AttestationError> {
            let user_data = doc_bytes
                .strip_prefix(Self::PREFIX)
                .ok_or_else(|| AttestationError::ParseFailed("missing mock prefix".into()))?
                .to_vec();

            Ok(VerifiedAttestation {
                vendor: Self::VENDOR,
                instance_id: "mock".into(),
                timestamp_ms: 0,
                measurement: Self::MEASUREMENT.to_vec(),
                user_data: Some(user_data),
                public_key: None,
            })
        }
    }
}

#[cfg(feature = "mock")]
pub use mock::MockAttestationVerifier;
