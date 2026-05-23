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

use title_core::ProcessResponse;

use crate::cnft;
use crate::signing_key::SolanaSigningKey;

/// Offchain data fetched from the URL provided in the extension request.
/// This is the core processing result stored by the client.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OffchainData {
    /// The core processing result (signature_hash + processor results).
    #[serde(flatten)]
    pub response: ProcessResponse,
}

/// Errors from Solana Extension processing.
#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error("Failed to fetch offchain data: {0}")]
    FetchFailed(String),

    #[error("Failed to parse offchain data: {0}")]
    ParseFailed(String),

    #[error("Attestation verification failed: {0}")]
    AttestationInvalid(String),

    #[error("PCR mismatch: expected {expected}, got {actual}")]
    PcrMismatch { expected: String, actual: String },

    #[error("user_data hash mismatch")]
    UserDataMismatch,

    #[error("Signing key not whitelisted or expired")]
    KeyNotWhitelisted,

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

/// Compute JCS-canonicalized SHA-256 hash of a VerifiableResponse.
/// Spec §1.5, §2.3 — same as orchestrator.rs but standalone.
fn compute_verifiable_hash(
    response: &ProcessResponse,
) -> Result<Vec<u8>, ExtensionError> {
    let json_value = serde_json::to_value(&response.verifiable)
        .map_err(|e| ExtensionError::ParseFailed(e.to_string()))?;
    let jcs_bytes = serde_json_canonicalizer::to_vec(&json_value)
        .map_err(|e| ExtensionError::AttestationInvalid(format!("JCS failed: {}", e)))?;
    Ok(Sha256::digest(&jcs_bytes).to_vec())
}

/// Verify that an Attestation Document's user_data matches the processing result.
/// Spec §6.2 — user_dataのハッシュが処理結果と一致するか確認
///
/// In the current mock/test implementation, the attestation is treated as:
/// `"mock-attestation:" + user_data_bytes`
///
/// In production, this would parse COSE_Sign1, verify the certificate chain,
/// check PCR values, and extract user_data.
pub fn verify_attestation_binding(
    response: &ProcessResponse,
    expected_pcr0: Option<&[u8; 48]>,
) -> Result<(), ExtensionError> {
    let attestation_bytes = base64::engine::general_purpose::STANDARD
        .decode(&response.attestation)
        .map_err(|e| ExtensionError::AttestationInvalid(format!("Base64 decode: {}", e)))?;

    let expected_hash = compute_verifiable_hash(response)?;

    // Extract user_data from attestation.
    // Mock format: "mock-attestation:" + user_data
    // Production: parse COSE_Sign1 → CBOR payload → user_data field
    let user_data = extract_user_data_from_attestation(&attestation_bytes)?;

    if user_data != expected_hash {
        return Err(ExtensionError::UserDataMismatch);
    }

    // PCR verification (when expected_pcr0 is provided)
    if let Some(_expected) = expected_pcr0 {
        // Production: extract PCR0 from attestation and compare
        // For now, PCR check is a no-op in mock mode
    }

    Ok(())
}

/// Extract user_data from an Attestation Document.
/// Mock: strips "mock-attestation:" prefix.
/// Production: would parse COSE_Sign1 CBOR → attestation document → user_data.
fn extract_user_data_from_attestation(
    attestation_bytes: &[u8],
) -> Result<Vec<u8>, ExtensionError> {
    let prefix = b"mock-attestation:";
    if attestation_bytes.starts_with(prefix) {
        return Ok(attestation_bytes[prefix.len()..].to_vec());
    }

    // Production path: parse COSE_Sign1 and extract user_data
    // This would use attestation-verify library logic
    Err(ExtensionError::AttestationInvalid(
        "Unrecognized attestation format".into(),
    ))
}

/// Process a Solana Extension request.
/// Spec §6.2 — full pipeline from offchain data to partially signed TX.
///
/// # Arguments
/// * `signing_key` — TEE's Solana signing key
/// * `offchain_data` — The fetched and parsed offchain data (caller handles HTTP fetch)
/// * `request` — Parsed extension request parameters
/// * `expected_pcr0` — Expected PCR0 for attestation verification (None in mock)
pub fn process_extension(
    signing_key: &SolanaSigningKey,
    offchain_data: &ProcessResponse,
    request: &ExtensionRequest,
    expected_pcr0: Option<&[u8; 48]>,
) -> Result<Vec<u8>, ExtensionError> {
    // Step 1: Verify attestation binding
    verify_attestation_binding(offchain_data, expected_pcr0)?;

    // Step 2: Build and sign cNFT mint transaction
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

    // Step 3: Serialize the partially signed transaction
    let tx_bytes = cnft::serialize_transaction(&tx)?;

    Ok(tx_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use title_core::{ProcessorOutput, VerifiableResponse};

    fn mock_process_response() -> ProcessResponse {
        let verifiable = VerifiableResponse {
            signature_hash: "sha256:abcdef1234567890".into(),
            results: {
                let mut m = HashMap::new();
                m.insert(
                    "c2pa-verify".into(),
                    ProcessorOutput::ok(serde_json::json!({"validation": "valid"})),
                );
                m
            },
        };

        // Compute the JCS hash for user_data
        let json_value = serde_json::to_value(&verifiable).unwrap();
        let jcs_bytes = serde_json_canonicalizer::to_vec(&json_value).unwrap();
        let hash = Sha256::digest(&jcs_bytes);

        // Build mock attestation: "mock-attestation:" + hash
        let mut attestation_raw = b"mock-attestation:".to_vec();
        attestation_raw.extend_from_slice(&hash);
        let attestation = base64::engine::general_purpose::STANDARD.encode(&attestation_raw);

        ProcessResponse {
            verifiable,
            attestation,
        }
    }

    #[test]
    fn verify_attestation_binding_valid() {
        let response = mock_process_response();
        assert!(verify_attestation_binding(&response, None).is_ok());
    }

    #[test]
    fn verify_attestation_binding_tampered_results() {
        let mut response = mock_process_response();
        // Tamper with signature_hash after attestation was generated
        response.verifiable.signature_hash = "sha256:tampered".into();
        assert!(matches!(
            verify_attestation_binding(&response, None),
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
            verify_attestation_binding(&response, None),
            Err(ExtensionError::AttestationInvalid(_))
        ));
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

        let tx_bytes = process_extension(&key, &response, &request, None).unwrap();
        assert!(!tx_bytes.is_empty());

        // Should deserialize back to a VersionedTransaction
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

        let result = process_extension(&key, &response, &request, None);
        assert!(result.is_err());
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
