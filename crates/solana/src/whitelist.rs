// SPDX-License-Identifier: Apache-2.0

//! # Whitelist PDA Types
//!
//! Spec §6.2 — Whitelist PDA account structure for on-chain key registration.
//!
//! A whitelist entry records a TEE signing key that has been verified
//! via ZK proof of Attestation Document. The entry has a 90-day expiry
//! for new mints, but cNFTs minted during the valid period remain valid
//! permanently.

use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Default signing key validity period: 90 days in seconds.
/// Spec §6.2 — 署名鍵の有効期限（目安: 90日）
pub const KEY_EXPIRY_SECONDS: i64 = 90 * 24 * 60 * 60;

/// Maximum length of a TEE measurement stored on-chain (bytes).
/// Mirrors the on-chain constant in `programs/title-whitelist`.
pub const MAX_MEASUREMENT_LEN: usize = 64;

/// On-chain whitelist entry for a TEE signing key.
/// Spec §6.2
///
/// Client-side mirror of the Anchor account in `programs/title-whitelist/`.
/// Used for PDA derivation and validity checks.
///
/// Seeds: `[b"whitelist", signing_pubkey.as_ref()]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhitelistEntry {
    /// The TEE's Ed25519 signing public key (32 bytes).
    pub signing_pubkey: [u8; 32],

    /// Unix timestamp when this key was registered.
    pub registered_at: i64,

    /// Unix timestamp when this key expires for new mints.
    /// Spec §6.2 — 有効期限を過ぎた署名鍵では新しいcNFTを発行できないが、
    /// 有効期限内に発行されたcNFTは永続的に有効。
    pub expires_at: i64,

    /// TEE measurement at the moment of registration (vendor-neutral).
    /// AWS Nitro emits a 48-byte measurement (PCR0); other vendors may differ.
    pub measurement: Vec<u8>,

    /// `true` once `revoke_key` has been called for this entry. Applications
    /// must treat a revoked key as untrusted even within its `expires_at`
    /// window. Spec §6.2 — TEE compromise emergency response.
    pub revoked: bool,

    /// PDA bump seed.
    pub bump: u8,
}

impl WhitelistEntry {
    /// Account data size (for rent calculation).
    /// discriminator(8) + pubkey(32) + registered_at(8) + expires_at(8)
    ///   + measurement(64 + 1) + revoked(1) + bump(1)
    pub const SIZE: usize = 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1;

    /// Check if this key is valid for new mints at the given timestamp.
    /// A revoked key is never valid regardless of expiry.
    pub fn is_valid_at(&self, unix_timestamp: i64) -> bool {
        !self.revoked && unix_timestamp < self.expires_at
    }

    /// Check if this key has expired at the given timestamp (does not count
    /// revocation; use `is_valid_at` for the combined check).
    pub fn is_expired_at(&self, unix_timestamp: i64) -> bool {
        unix_timestamp >= self.expires_at
    }
}

/// Title Protocol whitelist program ID.
/// Matches `declare_id!` in `programs/title-whitelist/src/lib.rs`.
pub fn whitelist_program_id() -> Pubkey {
    Pubkey::from_str("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs").unwrap()
}

/// Derive the whitelist PDA for a given signing public key.
/// Seeds: `[b"whitelist", signing_pubkey]`
pub fn derive_whitelist_pda(signing_pubkey: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"whitelist", signing_pubkey.as_ref()],
        &whitelist_program_id(),
    )
}

/// Derive the approved-vkeys PDA (singleton).
/// Seeds: `[b"approved_vkeys"]`
pub fn derive_approved_vkeys_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"approved_vkeys"], &whitelist_program_id())
}

/// Derive the approved-measurements PDA (singleton).
/// Seeds: `[b"approved_measurements"]`
pub fn derive_approved_measurements_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"approved_measurements"], &whitelist_program_id())
}

/// Solana program instructions for whitelist management.
/// Spec §6.2
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WhitelistInstruction {
    /// Initialise the approved-vkeys registry (called once by admin).
    InitializeApprovedVkeys,

    /// Add a verifying-key hash to the approved set (admin only).
    AddApprovedVkey { vkey_hash: Vec<u8> },

    /// Remove a verifying-key hash from the approved set (admin only).
    RemoveApprovedVkey { vkey_hash: Vec<u8> },

    /// Initialise the approved-measurements registry (called once by admin).
    InitializeApprovedMeasurements,

    /// Add a TEE measurement to the approved set (admin only).
    AddApprovedMeasurement { measurement: Vec<u8> },

    /// Remove a TEE measurement from the approved set (admin only).
    RemoveApprovedMeasurement { measurement: Vec<u8> },

    /// Register a new signing key with ZK proof verification.
    /// Spec §6.2 — vkey + measurement の二段照合 + proof 検証 + bind 確認
    RegisterKey {
        /// The signing public key to register (32 bytes).
        signing_pubkey: Vec<u8>,
        /// SP1 verification key hash (32 bytes) — identifies the guest program.
        sp1_vkey_hash: Vec<u8>,
        /// SP1 Groth16 proof bytes (~479 bytes).
        proof: Vec<u8>,
        /// SP1 public values bytes (Borsh-encoded, vendor-neutral layout).
        public_values: Vec<u8>,
    },

    /// Revoke a key from the whitelist (admin only). The entry's PDA stays
    /// in place with `revoked = true` so the original proof cannot be
    /// replayed to re-register the same key.
    /// Spec §6.2 — TEEの侵害等、特別な事情が発生した場合にのみ
    RevokeKey { signing_pubkey: Vec<u8> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_entry_validity() {
        let mut entry = WhitelistEntry {
            signing_pubkey: [1u8; 32],
            registered_at: 1000,
            expires_at: 1000 + KEY_EXPIRY_SECONDS,
            measurement: vec![0u8; 48],
            revoked: false,
            bump: 255,
        };

        assert!(entry.is_valid_at(1000));
        assert!(entry.is_valid_at(1000 + KEY_EXPIRY_SECONDS - 1));
        assert!(!entry.is_valid_at(1000 + KEY_EXPIRY_SECONDS));
        assert!(entry.is_expired_at(1000 + KEY_EXPIRY_SECONDS));
        assert!(!entry.is_valid_at(1000 + KEY_EXPIRY_SECONDS + 1));

        // A revoked key is never valid, even within its expiry window.
        entry.revoked = true;
        assert!(!entry.is_valid_at(1000));
        assert!(!entry.is_valid_at(1000 + KEY_EXPIRY_SECONDS - 1));
    }

    #[test]
    fn key_expiry_is_90_days() {
        assert_eq!(KEY_EXPIRY_SECONDS, 7_776_000);
    }

    #[test]
    fn whitelist_entry_size() {
        // discriminator(8) + pubkey(32) + registered_at(8) + expires_at(8)
        //   + measurement(MAX + 1) + revoked(1) + bump(1)
        assert_eq!(WhitelistEntry::SIZE, 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1);
    }

    #[test]
    fn derive_pda_deterministic() {
        let pubkey = [42u8; 32];
        let (pda1, bump1) = derive_whitelist_pda(&pubkey);
        let (pda2, bump2) = derive_whitelist_pda(&pubkey);
        assert_eq!(pda1, pda2);
        assert_eq!(bump1, bump2);
    }

    #[test]
    fn derive_pda_different_keys_different_pdas() {
        let (pda1, _) = derive_whitelist_pda(&[1u8; 32]);
        let (pda2, _) = derive_whitelist_pda(&[2u8; 32]);
        assert_ne!(pda1, pda2);
    }

    #[test]
    fn derive_approved_vkeys_pda_stable() {
        let (a, _) = derive_approved_vkeys_pda();
        let (b, _) = derive_approved_vkeys_pda();
        assert_eq!(a, b);
    }

    #[test]
    fn whitelist_instruction_serialize() {
        let ix = WhitelistInstruction::RegisterKey {
            signing_pubkey: vec![1u8; 32],
            sp1_vkey_hash: vec![2u8; 32],
            proof: vec![3u8; 479],
            public_values: vec![4u8; 169],
        };
        let json = serde_json::to_string(&ix).unwrap();
        let restored: WhitelistInstruction = serde_json::from_str(&json).unwrap();
        match restored {
            WhitelistInstruction::RegisterKey { signing_pubkey, sp1_vkey_hash, proof, public_values } => {
                assert_eq!(signing_pubkey, vec![1u8; 32]);
                assert_eq!(sp1_vkey_hash, vec![2u8; 32]);
                assert_eq!(proof.len(), 479);
                assert_eq!(public_values.len(), 169);
            }
            _ => panic!("Expected RegisterKey"),
        }
    }

    #[test]
    fn revoke_key_instruction_serialize() {
        let ix = WhitelistInstruction::RevokeKey {
            signing_pubkey: vec![99u8; 32],
        };
        let json = serde_json::to_string(&ix).unwrap();
        let restored: WhitelistInstruction = serde_json::from_str(&json).unwrap();
        match restored {
            WhitelistInstruction::RevokeKey { signing_pubkey } => {
                assert_eq!(signing_pubkey, vec![99u8; 32]);
            }
            _ => panic!("Expected RevokeKey"),
        }
    }
}
