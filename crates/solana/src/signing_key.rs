// SPDX-License-Identifier: Apache-2.0

//! # Ed25519 Signing Key for Solana Extension
//!
//! Spec §6.2 — TEE generates an Ed25519 keypair at startup.
//! The secret key lives only in TEE memory.
//! The public key is exposed via GET /solana-keys (Base58).

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;

/// Solana Ed25519 signing keypair held in TEE memory.
/// Spec §6.2 — secret key never leaves TEE.
pub struct SolanaSigningKey {
    signing_key: SigningKey,
}

impl SolanaSigningKey {
    /// Generate a new keypair from a cryptographic RNG.
    /// Spec §6.2 — generated at TEE startup.
    pub fn generate(rng: &mut (impl rand::RngCore + rand::CryptoRng)) -> Self {
        let signing_key = SigningKey::generate(rng);
        Self { signing_key }
    }

    /// Reconstruct from raw 32-byte seed (for testing only).
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(seed);
        Self { signing_key }
    }

    /// Returns the Ed25519 public key as a Solana Pubkey.
    pub fn pubkey(&self) -> Pubkey {
        Pubkey::new_from_array(self.verifying_key().to_bytes())
    }

    /// Returns the Ed25519 public key as Base58 string.
    /// Spec §2.5 — GET /solana-keys returns this value.
    pub fn pubkey_base58(&self) -> String {
        self.pubkey().to_string()
    }

    /// Returns the raw Ed25519 verifying (public) key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Returns the raw 32-byte public key bytes.
    pub fn pubkey_bytes(&self) -> [u8; 32] {
        self.verifying_key().to_bytes()
    }

    /// SHA-256 hash of the public key.
    /// Spec §6.2 — used as user_data in Attestation Document:
    /// user_data = SHA-256(Solana公開鍵)
    pub fn pubkey_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.pubkey_bytes());
        hasher.finalize().into()
    }

    /// Sign a message with the Ed25519 secret key.
    /// Used for partial signing of Solana transactions.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }

    /// Sign a Solana VersionedTransaction message and apply partial signature.
    ///
    /// In-house callers (`build_v0_tx` in `cnft.rs`) always present a `tx`
    /// whose `signatures` slot count matches `num_required_signatures`, so
    /// the slot for our pubkey is guaranteed to exist. But this entry point
    /// is also exposed to future SDKs / external signers that may hand us a
    /// `VersionedTransaction` reconstructed from off-host bytes. A
    /// `signatures.len() < num_required_signatures` mismatch in that case
    /// must surface as a clean `Err`, not an out-of-bounds panic.
    pub fn sign_transaction(
        &self,
        tx: &mut solana_sdk::transaction::VersionedTransaction,
    ) -> Result<(), SigningKeyError> {
        let message_bytes = tx.message.serialize();
        let sig_bytes = self.sign(&message_bytes);

        let pubkey = self.pubkey();
        let num_signers = tx.message.header().num_required_signatures as usize;
        let static_keys = tx.message.static_account_keys();

        let index = static_keys
            .iter()
            .take(num_signers)
            .position(|k| k == &pubkey)
            .ok_or_else(|| SigningKeyError::PubkeyNotInSigners(pubkey.to_string()))?;

        if tx.signatures.len() <= index {
            return Err(SigningKeyError::SignatureSlotMissing {
                index,
                len: tx.signatures.len(),
            });
        }
        tx.signatures[index] = solana_sdk::signature::Signature::from(sig_bytes);
        Ok(())
    }
}

/// Errors from signing key operations.
#[derive(Debug, thiserror::Error)]
pub enum SigningKeyError {
    #[error("Public key {0} not found in transaction signers")]
    PubkeyNotInSigners(String),

    /// The transaction's `signatures` vec is shorter than the signer slot
    /// our pubkey resolves to. Solana's wire format normally guarantees
    /// `signatures.len() == num_required_signatures`, but a malformed
    /// `VersionedTransaction` deserialised from untrusted bytes can violate
    /// it. Reject the request rather than panicking on the slot write.
    #[error(
        "transaction signature slot {index} missing (only {len} slots present)"
    )]
    SignatureSlotMissing { index: usize, len: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn generate_and_sign() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let msg = b"hello title protocol";
        let sig = key.sign(msg);

        let vk = key.verifying_key();
        let signature = ed25519_dalek::Signature::from_bytes(&sig);
        assert!(vk.verify(msg, &signature).is_ok());
    }

    #[test]
    fn from_seed_deterministic() {
        let seed = [42u8; 32];
        let k1 = SolanaSigningKey::from_seed(&seed);
        let k2 = SolanaSigningKey::from_seed(&seed);
        assert_eq!(k1.pubkey_bytes(), k2.pubkey_bytes());
    }

    #[test]
    fn pubkey_is_32_bytes() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        assert_eq!(key.pubkey_bytes().len(), 32);
    }

    #[test]
    fn pubkey_base58_nonempty() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let b58 = key.pubkey_base58();
        assert!(!b58.is_empty());
        // Solana Base58 pubkeys are 32-44 chars
        assert!(b58.len() >= 32 && b58.len() <= 44);
    }

    #[test]
    fn pubkey_hash_is_sha256() {
        let seed = [1u8; 32];
        let key = SolanaSigningKey::from_seed(&seed);
        let hash = key.pubkey_hash();
        assert_eq!(hash.len(), 32);

        // Verify manually
        let mut hasher = Sha256::new();
        hasher.update(key.pubkey_bytes());
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(hash, expected);
    }

    #[test]
    fn pubkey_matches_solana_pubkey() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let solana_pk = key.pubkey();
        assert_eq!(solana_pk.to_bytes(), key.pubkey_bytes());
    }

    /// Regression: a `VersionedTransaction` whose `signatures` vec was
    /// truncated (e.g. malformed off-host input) must not panic the slot
    /// write — `sign_transaction` is the boundary where untrusted bytes
    /// meet our signing key, so the failure has to be a clean `Err`.
    /// Originally reported in github.com/yudai-mori-2004/title-protocol PR #1
    /// (dakewamama).
    #[test]
    fn sign_transaction_rejects_truncated_signature_slots() {
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0, VersionedMessage};
        use solana_sdk::system_instruction;
        use solana_sdk::transaction::VersionedTransaction;

        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let payer = key.pubkey();
        let recipient = solana_sdk::pubkey::Pubkey::new_unique();

        let ix = system_instruction::transfer(&payer, &recipient, 1);
        let msg = v0::Message::try_compile(&payer, &[ix], &[], Hash::new_unique())
            .expect("compile v0 message");

        // Construct a deliberately malformed transaction: the header claims
        // one required signer but the signatures vec is empty.
        let mut tx = VersionedTransaction {
            signatures: Vec::new(),
            message: VersionedMessage::V0(msg),
        };

        let err = key
            .sign_transaction(&mut tx)
            .expect_err("signing must reject truncated signature slots");
        match err {
            SigningKeyError::SignatureSlotMissing { index, len } => {
                assert_eq!(index, 0);
                assert_eq!(len, 0);
            }
            other => panic!("expected SignatureSlotMissing, got {other:?}"),
        }
    }
}
