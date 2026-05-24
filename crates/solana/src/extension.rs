// SPDX-License-Identifier: Apache-2.0

//! # Solana Extension Orchestration
//!
//! Spec §6.1, §6.2
//!
//! Handles POST /extension/solana requests in the TEE:
//! 1. Fetch offchain data (core processing result + Attestation Document)
//! 2. Verify the Attestation Document:
//!    - Certificate chain validity
//!    - PCR values match our own code
//!    - user_data hash matches the processing result
//! 3. Build cNFT mint transaction
//! 4. Partially sign with TEE signing key
//! 5. Return the partially signed transaction

use base64::Engine;
use sha2::{Digest, Sha256};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;

use title_attestation::{AttestationVerifier, VerifiedAttestation};
use title_core::ProcessResponse;

use crate::cnft;
use crate::signing_key::SolanaSigningKey;

/// Errors from Solana Extension processing.
#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error("Failed to parse offchain data: {0}")]
    ParseFailed(String),

    #[error("Attestation verification failed: {0}")]
    AttestationInvalid(String),

    #[error("TEE measurement mismatch: expected {expected}, got {actual}")]
    MeasurementMismatch { expected: String, actual: String },

    #[error("user_data hash mismatch")]
    UserDataMismatch,

    #[error("Transaction construction failed: {0}")]
    TxFailed(#[from] cnft::CnftError),

    #[error("Base58 decode failed: {0}")]
    Base58Failed(String),
}

/// Solana Extension request parameters.
/// Spec §6.2
#[derive(Debug, Clone)]
pub struct ExtensionRequest {
    /// URL of the offchain data (core processing result).
    pub offchain_data_url: String,
    /// Collection address (Base58). Optional — developer chooses whether to
    /// group cNFTs under a collection. Not part of the trust model.
    pub collection: Option<Pubkey>,
    /// Merkle tree address (Base58).
    pub merkle_tree: Pubkey,
    /// Recent blockhash (Base58).
    pub recent_blockhash: Hash,
    /// Payer / leaf owner address.
    pub payer: Pubkey,
}

impl ExtensionRequest {
    /// Parse from raw string fields (as received from the Gateway API).
    pub fn from_strings(
        offchain_data_url: &str,
        collection: Option<&str>,
        merkle_tree: &str,
        recent_blockhash: &str,
        payer: &str,
    ) -> Result<Self, ExtensionError> {
        Ok(Self {
            offchain_data_url: offchain_data_url.to_string(),
            collection: collection.map(parse_pubkey).transpose()?,
            merkle_tree: parse_pubkey(merkle_tree)?,
            recent_blockhash: parse_hash(recent_blockhash)?,
            payer: parse_pubkey(payer)?,
        })
    }
}

fn parse_pubkey(s: &str) -> Result<Pubkey, ExtensionError> {
    s.parse::<Pubkey>()
        .map_err(|e| ExtensionError::Base58Failed(format!("invalid pubkey '{}': {}", s, e)))
}

fn parse_hash(s: &str) -> Result<Hash, ExtensionError> {
    s.parse::<Hash>()
        .map_err(|e| ExtensionError::Base58Failed(format!("invalid hash '{}': {}", s, e)))
}

/// 仕様 §1.7 / §2.3 — core 処理用 user_data のドメインタグ。
/// `crates/tee/src/orchestrator.rs` の `CORE_USER_DATA_TAG` と完全一致させること。
const CORE_USER_DATA_TAG: &[u8] = b"title:core";

/// VerifiableResponse の JCS 正規化 + ドメインタグ付き SHA-256 を計算する。
/// orchestrator.rs の `compute_jcs_hash` と同じ式:
///   user_data = SHA-256(b"title:core" || JCS({"signature_hash":..., "results":...}))
fn compute_verifiable_hash(response: &ProcessResponse) -> Result<Vec<u8>, ExtensionError> {
    let json_value = serde_json::to_value(&response.verifiable)
        .map_err(|e| ExtensionError::ParseFailed(e.to_string()))?;
    let jcs_bytes = serde_json_canonicalizer::to_vec(&json_value)
        .map_err(|e| ExtensionError::AttestationInvalid(format!("JCS failed: {}", e)))?;
    let mut hasher = Sha256::new();
    hasher.update(CORE_USER_DATA_TAG);
    hasher.update(&jcs_bytes);
    Ok(hasher.finalize().to_vec())
}

/// Verify that an Attestation Document is authentic and binds to the processing result.
/// Spec §6.2
///
/// 1. Decode the Base64 attestation
/// 2. Run the vendor-specific verifier (cert chain, signature, validity period)
/// 3. Check `user_data == SHA-256(JCS(verifiable_response))`
/// 4. If `expected_measurement` is given, check it equals `verified.measurement`
pub fn verify_attestation_binding(
    verifier: &dyn AttestationVerifier,
    response: &ProcessResponse,
    expected_measurement: Option<&[u8]>,
    now_unix_secs: u64,
) -> Result<VerifiedAttestation, ExtensionError> {
    let attestation_bytes = base64::engine::general_purpose::STANDARD
        .decode(&response.attestation)
        .map_err(|e| ExtensionError::AttestationInvalid(format!("Base64 decode: {}", e)))?;

    let verified = verifier
        .verify(&attestation_bytes, now_unix_secs)
        .map_err(|e| ExtensionError::AttestationInvalid(e.to_string()))?;

    let expected_hash = compute_verifiable_hash(response)?;
    let user_data = verified
        .user_data
        .as_deref()
        .ok_or_else(|| ExtensionError::AttestationInvalid("attestation has no user_data".into()))?;
    if user_data != expected_hash {
        return Err(ExtensionError::UserDataMismatch);
    }

    if let Some(expected) = expected_measurement {
        if verified.measurement != expected {
            return Err(ExtensionError::MeasurementMismatch {
                expected: hex::encode(expected),
                actual: hex::encode(&verified.measurement),
            });
        }
    }

    Ok(verified)
}

/// Process a Solana Extension request end-to-end.
/// Spec §6.2 — fetch ⇒ verify attestation ⇒ build cNFT mint TX ⇒ partial sign.
///
/// The caller injects the `AttestationVerifier` matching the TEE vendor.
pub fn process_extension(
    verifier: &dyn AttestationVerifier,
    signing_key: &SolanaSigningKey,
    offchain_data: &ProcessResponse,
    request: &ExtensionRequest,
    expected_measurement: Option<&[u8]>,
    now_unix_secs: u64,
) -> Result<Vec<u8>, ExtensionError> {
    verify_attestation_binding(verifier, offchain_data, expected_measurement, now_unix_secs)?;

    let tx = cnft::build_and_sign_mint_tx(
        signing_key,
        &request.merkle_tree,
        &request.payer,
        &offchain_data.verifiable.signature_hash,
        &request.offchain_data_url,
        request.collection.as_ref(),
        &request.payer,
        &request.recent_blockhash,
    )?;

    let tx_bytes = cnft::serialize_transaction(&tx)?;
    Ok(tx_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use title_attestation::MockAttestationVerifier;
    use title_core::{ProcessorOutput, VerifiableResponse};

    fn mock_process_response() -> ProcessResponse {
        let verifiable = VerifiableResponse {
            signature_hash: "sha256:abcdef1234567890".into(),
            results: {
                let mut m = HashMap::new();
                m.insert(
                    "c2pa-verify".into(),
                    ProcessorOutput::from_value_object(serde_json::json!({"validation": "valid"})),
                );
                m
            },
        };

        let json_value = serde_json::to_value(&verifiable).unwrap();
        let jcs_bytes = serde_json_canonicalizer::to_vec(&json_value).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(CORE_USER_DATA_TAG);
        hasher.update(&jcs_bytes);
        let hash = hasher.finalize();

        let mut attestation_raw = MockAttestationVerifier::PREFIX.to_vec();
        attestation_raw.extend_from_slice(&hash);
        let attestation = base64::engine::general_purpose::STANDARD.encode(&attestation_raw);

        ProcessResponse {
            verifiable,
            attestation,
        }
    }

    fn verifier() -> MockAttestationVerifier {
        MockAttestationVerifier::new()
    }

    #[test]
    fn verify_attestation_binding_valid() {
        let response = mock_process_response();
        let verified = verify_attestation_binding(&verifier(), &response, None, 0).unwrap();
        assert_eq!(verified.vendor, "mock");
    }

    #[test]
    fn verify_attestation_binding_tampered_results() {
        let mut response = mock_process_response();
        response.verifiable.signature_hash = "sha256:tampered".into();
        assert!(matches!(
            verify_attestation_binding(&verifier(), &response, None, 0),
            Err(ExtensionError::UserDataMismatch)
        ));
    }

    #[test]
    fn verify_attestation_binding_bad_base64() {
        let response = ProcessResponse {
            verifiable: VerifiableResponse {
                signature_hash: "sha256:test".into(),
                results: HashMap::new(),
            },
            attestation: "not-valid-base64!!!".into(),
        };
        assert!(matches!(
            verify_attestation_binding(&verifier(), &response, None, 0),
            Err(ExtensionError::AttestationInvalid(_))
        ));
    }

    #[test]
    fn verify_attestation_binding_measurement_mismatch() {
        let response = mock_process_response();
        let wrong = [0xAAu8; 48];
        assert!(matches!(
            verify_attestation_binding(&verifier(), &response, Some(&wrong), 0),
            Err(ExtensionError::MeasurementMismatch { .. })
        ));
    }

    #[test]
    fn verify_attestation_binding_measurement_match() {
        let response = mock_process_response();
        // MockAttestationVerifier emits an all-zero 48-byte measurement.
        let expected = MockAttestationVerifier::MEASUREMENT;
        assert!(verify_attestation_binding(&verifier(), &response, Some(&expected), 0).is_ok());
    }

    #[test]
    fn process_extension_full_pipeline() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let response = mock_process_response();

        let request = ExtensionRequest {
            offchain_data_url: "https://example.com/output/abc123.json".into(),
            collection: Some(Pubkey::new_unique()),
            merkle_tree: Pubkey::new_unique(),
            recent_blockhash: Hash::new_unique(),
            payer: Pubkey::new_unique(),
        };

        let tx_bytes = process_extension(&verifier(), &key, &response, &request, None, 0).unwrap();
        assert!(!tx_bytes.is_empty());

        let tx: solana_sdk::transaction::VersionedTransaction =
            bincode::deserialize(&tx_bytes).unwrap();
        assert!(!tx.signatures.is_empty());
    }

    #[test]
    fn process_extension_rejects_tampered() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let mut response = mock_process_response();
        response.verifiable.signature_hash = "sha256:tampered".into();

        let request = ExtensionRequest {
            offchain_data_url: "https://example.com/output/abc123.json".into(),
            collection: None,
            merkle_tree: Pubkey::new_unique(),
            recent_blockhash: Hash::new_unique(),
            payer: Pubkey::new_unique(),
        };

        let result = process_extension(&verifier(), &key, &response, &request, None, 0);
        // signature_hash 改ざんは user_data binding (Spec §6.2 確認 3) で
        // 弾かれる経路を pin する。「単に Err になる」だけでなく、どの
        // variant が発火したかを固定して、将来 verify_attestation_binding
        // の戻り値を別 variant にすり替えるリグレッションを catch する。
        assert!(matches!(result, Err(ExtensionError::UserDataMismatch)));
    }

    #[test]
    fn parse_pubkey_valid() {
        let pk = Pubkey::new_unique();
        let parsed = parse_pubkey(&pk.to_string()).unwrap();
        assert_eq!(pk, parsed);
    }

    #[test]
    fn parse_pubkey_invalid() {
        assert!(parse_pubkey("not-a-pubkey").is_err());
    }

    #[test]
    fn parse_hash_valid() {
        let h = Hash::new_unique();
        let parsed = parse_hash(&h.to_string()).unwrap();
        assert_eq!(h, parsed);
    }
}
