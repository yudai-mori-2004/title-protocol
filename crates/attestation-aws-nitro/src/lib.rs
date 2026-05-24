// SPDX-License-Identifier: Apache-2.0

//! # Title Attestation: AWS Nitro
//!
//! Implements [`AttestationVerifier`] for AWS Nitro Enclaves: parses the
//! COSE_Sign1 envelope, verifies the embedded certificate chain against the
//! pinned AWS Nitro root, and surfaces the PCR0 measurement plus the
//! `user_data` / `public_key` fields.
//
// Derived from Automata Network's aws-nitro-enclave-attestation (Apache-2.0).

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

// `authenticate()` の戻り値型として外部に露出する。downstream で
// leaf cert やチェーン情報を inspect する用途を想定した API surface
// であり、現状は内部利用のみ。
pub use cert::CertChain;
pub use doc::{AttestationDocument, AttestationReport};
pub use sign::{PubKey, SigAlgo};

use serde_bytes::ByteBuf;
use title_attestation::{AttestationError, AttestationVerifier, VerifiedAttestation};

/// Vendor tag for `VerifiedAttestation::vendor` and `AttestationVerifier::vendor`.
pub const VENDOR: &str = "aws-nitro";

/// AWS Nitro Enclave Attestation Document verifier.
///
/// The chain root is pinned to [`constants::AWS_NITRO_ROOT_CA_SHA256`]; a
/// cabundle whose first certificate does not match the fingerprint is
/// rejected.
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

        report
            .authenticate(now_unix_secs)
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
    fn rejects_invalid_bytes() {
        let v = AwsNitroVerifier::new();
        let err = v.verify(b"not a valid attestation", 0).unwrap_err();
        assert!(matches!(err, AttestationError::ParseFailed(_)));
    }

    /// End-to-end verification against a real AWS Nitro Attestation Document.
    /// Document captured from a live Nitro Enclave; stored alongside this crate
    /// so tests don't depend on anything outside the crate tree.
    #[test]
    fn verifies_real_aws_nitro_attestation() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/attestation_1.report"
        );
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

    /// A verifier clock set before the document's own timestamp must be
    /// rejected — the trait contract calls this out explicitly.
    #[test]
    fn rejects_doc_timestamp_in_future() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/attestation_1.report"
        );
        let doc_bytes = std::fs::read(path).expect("fixture must exist alongside the crate");
        let report = AttestationReport::parse(&doc_bytes).unwrap();
        let doc_ts_secs = report.doc().timestamp / 1000;

        let v = AwsNitroVerifier::new();
        let err = v.verify(&doc_bytes, doc_ts_secs - 60).unwrap_err();
        assert!(matches!(err, AttestationError::SignatureInvalid(_)));
    }
}
