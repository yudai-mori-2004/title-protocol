// SPDX-License-Identifier: Apache-2.0

//! # Whitelist PDA Types
//!
//! Spec §6.2 — Whitelist PDA account structure for on-chain key registration.
//!
//! A whitelist entry records a TEE signing key that has been verified
//! via ZK proof of Attestation Document. The entry has a 90-day expiry
//! for new mints, but cNFTs minted during the valid period remain valid
//! permanently.

use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

/// Signing key validity period mirrored from the on-chain program.
///
/// **Authoritative source is on-chain** (`programs/title-whitelist`); this
/// constant is for client-side `is_valid_at` checks and rotates with the
/// program. Anchor's `idl` flow does not expose plain constants, so this
/// duplicates the on-chain definition — update both together.
pub const KEY_EXPIRY_SECONDS: i64 = 90 * 24 * 60 * 60;

/// On-chain `StoredMeasurement` layout: a fixed 64-byte buffer plus a
/// length byte that tells callers how much of `bytes` is meaningful.
/// Mirrors `programs/title-whitelist::StoredMeasurement` exactly so a
/// future `AnchorDeserialize` flow walks the same wire bytes.
///
/// **Authoritative source is on-chain `MAX_MEASUREMENT_LEN = 64`** in
/// `programs/title-whitelist/src/lib.rs`. このバッファ長を変更する際は
/// 両方を同時に更新すること (host crate からは program crate を path 依存
/// できないためハードコード必須)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMeasurement {
    pub bytes: [u8; 64],
    pub len: u8,
}

impl StoredMeasurement {
    /// Wrap a vendor-emitted measurement (e.g. 48-byte SHA-384 PCR0).
    /// Panics if `bytes.len() > 64`.
    pub fn from_slice(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() <= 64,
            "measurement length {} exceeds storage capacity 64",
            bytes.len()
        );
        let mut buf = [0u8; 64];
        buf[..bytes.len()].copy_from_slice(bytes);
        Self {
            bytes: buf,
            len: bytes.len() as u8,
        }
    }

    /// Meaningful portion of `bytes` (length-prefixed).
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// On-chain whitelist entry for a TEE signing key. Spec §6.2.
///
/// Mirrors the Anchor account in `programs/title-whitelist/`.
/// Seeds: `[b"whitelist", signing_pubkey.as_ref()]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhitelistEntry {
    pub signing_pubkey: [u8; 32],
    pub registered_at: i64,
    pub expires_at: i64,
    pub measurement: StoredMeasurement,
    /// Set to `true` once `revoke_key` has been called; the PDA stays in
    /// place so the original proof cannot re-register the same key.
    /// Applications must treat a revoked key as untrusted even within its
    /// `expires_at` window.
    pub revoked: bool,
    pub bump: u8,
}

impl WhitelistEntry {
    /// Account data size: 8 (discriminator) + on-chain layout.
    /// discriminator(8) + signing_pubkey(32) + registered_at(8) + expires_at(8)
    ///   + measurement(64 + 1) + revoked(1) + bump(1)
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 64 + 1 + 1 + 1;

    pub fn is_valid_at(&self, unix_timestamp: i64) -> bool {
        !self.revoked && unix_timestamp < self.expires_at
    }

    pub fn is_expired_at(&self, unix_timestamp: i64) -> bool {
        unix_timestamp >= self.expires_at
    }
}

/// Title Protocol whitelist program ID.
/// Matches `declare_id!` in `programs/title-whitelist/src/lib.rs`.
pub const WHITELIST_PROGRAM_ID: Pubkey = pubkey!("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs");

/// Derive the whitelist PDA for a given signing public key.
pub fn derive_whitelist_pda(signing_pubkey: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"whitelist", signing_pubkey.as_ref()],
        &WHITELIST_PROGRAM_ID,
    )
}

/// Derive the approved-vkeys PDA (singleton).
pub fn derive_approved_vkeys_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"approved_vkeys"], &WHITELIST_PROGRAM_ID)
}

/// Derive the approved-measurements PDA (singleton).
pub fn derive_approved_measurements_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"approved_measurements"], &WHITELIST_PROGRAM_ID)
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
            measurement: StoredMeasurement::from_slice(&[0u8; 48]),
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
    fn stored_measurement_roundtrip() {
        let m = StoredMeasurement::from_slice(&[0xAB; 48]);
        assert_eq!(m.len, 48);
        assert_eq!(m.as_bytes(), &[0xAB; 48][..]);
        // Zero-padded tail
        assert_eq!(&m.bytes[48..], &[0u8; 16][..]);
    }

    #[test]
    #[should_panic(expected = "exceeds storage capacity")]
    fn stored_measurement_rejects_oversized() {
        StoredMeasurement::from_slice(&[0u8; 65]);
    }

    #[test]
    fn whitelist_entry_size_matches_on_chain_layout() {
        // Anchor on-chain layout: discriminator + fields
        let expected = 8                    // discriminator
            + 32                            // signing_pubkey
            + 8                             // registered_at
            + 8                             // expires_at
            + 64 + 1                        // StoredMeasurement bytes + len
            + 1                             // revoked
            + 1; // bump
        assert_eq!(WhitelistEntry::SIZE, expected);
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
}
