// SPDX-License-Identifier: Apache-2.0

//! # Title Attestation: AWS Nitro
//!
//! Spec §5.2, §6.2
//!
//! Implements `title_attestation::AttestationVerifier` for AWS Nitro Enclaves.
//!
//! Parses the COSE_Sign1 envelope, walks the certificate chain back to the
//! embedded `cabundle` root, verifies every signature with RustCrypto, then
//! extracts the measurement (PCR0), `user_data`, and `public_key` fields.
//!
//! ## Origin
//!
//! Verification logic is derived from Automata Network's
//! `aws-nitro-enclave-attestation` crate (Apache-2.0), itself based on
//! Amazon's `aws-nitro-enclaves-cose` (Apache-2.0). The crypto primitives
//! were ported from OpenSSL to RustCrypto by Automata. Title Protocol
//! internalised the code and removed unrelated dependencies.

// When built for SP1, shadow the standard `sha2` and `p256` crates with
// SP1-precompile-accelerated forks. The rest of the source is untouched.
#[cfg(feature = "sp1")]
extern crate sha2_sp1 as sha2;

#[cfg(feature = "sp1")]
extern crate p256_sp1 as p256;

mod cert;
mod constants;
mod cose;
mod doc;
mod sign;

pub use cert::CertChain;
pub use doc::{AttestationDocument, AttestationReport};
pub use sign::{PubKey, SigAlgo};

use serde_bytes::ByteBuf;
use title_attestation::{AttestationError, AttestationVerifier, VerifiedAttestation};

/// Vendor tag for `VerifiedAttestation::vendor` and `AttestationVerifier::vendor`.
pub const VENDOR: &str = "aws-nitro";

/// AWS Nitro Enclave Attestation Document verifier.
///
/// Uses the certificate chain shipped inside each Attestation Document
/// (`cabundle`) and trusts it implicitly — AWS rotates the root externally
/// and includes the full chain in every document. Verifiers that require a
/// pinned root should re-check `cert_chain.certs[0]` against their own
/// trusted copy of the AWS Nitro root.
#[derive(Debug, Default, Clone)]
pub struct AwsNitroVerifier;

impl AwsNitroVerifier {
    pub fn new() -> Self {
        Self
    }
}

impl AttestationVerifier for AwsNitroVerifier {
    fn vendor(&self) -> &'static str {
        VENDOR
    }

    fn verify(
        &self,
        doc_bytes: &[u8],
        now_unix_secs: u64,
    ) -> Result<VerifiedAttestation, AttestationError> {
        let report = AttestationReport::parse(doc_bytes)
            .map_err(|e| AttestationError::ParseFailed(format!("{e:?}")))?;

        // Use the smaller of (now, doc.timestamp/1000) for cert validity, so
        // documents from a TEE whose clock is slightly ahead of ours still
        // verify while genuinely expired certificates are still caught.
        let check_ts = report.doc().timestamp.saturating_div(1000).min(now_unix_secs);

        report
            .authenticate(0, check_ts)
            .map_err(|e| AttestationError::SignatureInvalid(format!("{e:?}")))?;

        let mut doc = report.into_doc();

        let measurement = doc
            .pcrs
            .remove(&0)
            .ok_or_else(|| AttestationError::MissingField("PCR0".into()))?
            .into_array()
            .to_vec();

        Ok(VerifiedAttestation {
            vendor: VENDOR,
            instance_id: doc.module_id,
            timestamp_ms: doc.timestamp,
            measurement,
            user_data: doc.user_data.map(ByteBuf::into_vec),
            public_key: doc.public_key.map(ByteBuf::into_vec),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_tag_consistent() {
        let v = AwsNitroVerifier::new();
        assert_eq!(v.vendor(), VENDOR);
        assert_eq!(VENDOR, "aws-nitro");
    }

    #[test]
    fn rejects_invalid_bytes() {
        let v = AwsNitroVerifier::new();
        let err = v.verify(b"not a valid attestation", 0).unwrap_err();
        matches!(err, AttestationError::ParseFailed(_));
    }

    /// End-to-end verification against a real AWS Nitro Attestation Document.
    /// Document captured from a live Nitro Enclave; stored alongside this crate
    /// so tests don't depend on anything outside the crate tree.
    #[test]
    fn verifies_real_aws_nitro_attestation() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/attestation_1.report");
        let doc_bytes = std::fs::read(path).expect("fixture must exist alongside the crate");

        let v = AwsNitroVerifier::new();
        // Use the doc's own timestamp so cert validity passes regardless of wall clock.
        let report = AttestationReport::parse(&doc_bytes).unwrap();
        let check_ts = report.doc().timestamp / 1000;

        let verified = v.verify(&doc_bytes, check_ts).expect("verify must succeed");

        assert_eq!(verified.vendor, "aws-nitro");
        assert!(!verified.instance_id.is_empty());
        assert_eq!(verified.measurement.len(), 48);
        assert!(verified.timestamp_ms > 0);
    }
}
