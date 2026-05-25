// SPDX-License-Identifier: Apache-2.0

//! Solana utilities — keypair loading, transaction signing, broadcast.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::VersionedTransaction;

/// Load a Solana keypair from a JSON file.
/// Format: JSON array of 64 u8 values (32 seed + 32 pubkey).
pub fn load_keypair(path: &str) -> Result<SigningKey, String> {
    let data =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read keypair: {e}"))?;
    let bytes: Vec<u8> =
        serde_json::from_str(&data).map_err(|e| format!("failed to parse keypair JSON: {e}"))?;
    if bytes.len() != 64 {
        return Err(format!("keypair must be 64 bytes, got {}", bytes.len()));
    }
    let seed: [u8; 32] = bytes[..32].try_into().unwrap();
    Ok(SigningKey::from_bytes(&seed))
}

/// Get the Solana Pubkey from an ed25519-dalek signing key.
pub fn pubkey(key: &SigningKey) -> Pubkey {
    Pubkey::new_from_array(key.verifying_key().to_bytes())
}

/// Sign a VersionedTransaction with an ed25519-dalek signing key.
pub fn sign_transaction(tx: &mut VersionedTransaction, key: &SigningKey) -> Result<(), String> {
    let pk = pubkey(key);
    let message_bytes = tx.message.serialize();
    let sig = key.sign(&message_bytes);

    let num_signers = tx.message.header().num_required_signatures as usize;
    let static_keys = tx.message.static_account_keys();

    let index = static_keys
        .iter()
        .take(num_signers)
        .position(|k| k == &pk)
        .ok_or_else(|| format!("pubkey {} not found in transaction signers", pk))?;

    if tx.signatures.len() <= index {
        return Err(format!("signature slot {} missing", index));
    }

    tx.signatures[index] = Signature::from(sig.to_bytes());
    Ok(())
}

/// Deserialize a base64-encoded VersionedTransaction.
pub fn decode_partial_tx(b64: &str) -> Result<VersionedTransaction, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    bincode::deserialize(&bytes).map_err(|e| format!("tx deserialize failed: {e}"))
}

/// Get the latest blockhash from a Solana RPC.
pub fn get_blockhash(rpc_url: &str) -> Result<solana_sdk::hash::Hash, String> {
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
    client
        .get_latest_blockhash()
        .map_err(|e| format!("failed to get blockhash: {e}"))
}

/// Broadcast a signed transaction and confirm.
pub fn broadcast(rpc_url: &str, tx: &VersionedTransaction) -> Result<Signature, String> {
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
    client
        .send_and_confirm_transaction(tx)
        .map_err(|e| format!("broadcast failed: {e}"))
}
