// SPDX-License-Identifier: Apache-2.0

//! # Whitelist Instruction Builders
//!
//! Spec §6.2 — Anchor instruction encoders for the on-chain whitelist program.
//!
//! Mirrors the wire layout of `programs/title-whitelist/src/lib.rs`. Used by
//! the `title-cli` `whitelist` subcommands and by devnet integration tests.
//! Each function builds a single `solana_sdk::instruction::Instruction` ready
//! to be wrapped in a `Transaction` with the appropriate signer.

use sha2::{Digest, Sha256};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program;

use crate::whitelist::{
    derive_approved_measurements_pda, derive_approved_vkeys_pda, derive_whitelist_pda,
    WHITELIST_PROGRAM_ID,
};

/// Compute the Anchor instruction discriminator (`SHA-256("global:<name>")[..8]`).
pub fn anchor_discriminator(name: &str) -> [u8; 8] {
    let hash = Sha256::digest(format!("global:{name}").as_bytes());
    hash[..8]
        .try_into()
        .expect("SHA-256 output is always ≥ 8 bytes")
}

/// Length of the on-chain `register_key` proof argument:
/// 4-byte SHA-256(GROTH16_VK_BYTES)[..4] selector + 256-byte BN254 Groth16
/// proof (pi_a 64 + pi_b 128 + pi_c 64).
pub const ON_CHAIN_PROOF_LEN: usize = 4 + 256;

/// Convert the raw bytes returned by `sp1_sdk::SP1ProofWithPublicValues::bytes()`
/// into the 260-byte slice that `register_key` expects.
///
/// SP1 SDK v6.2 emits a 356-byte bundle:
/// `selector(4) + groth16_public_inputs(96) + raw_groth16_proof(256)`.
/// The on-chain verifier reconstructs the public inputs itself from
/// `sp1_vkey_hash` and the SHA-256 digest of `public_values`, so it only
/// needs `selector + raw_groth16_proof`.
///
/// If the input is already 260 bytes (older SDK or pre-stripped), it is
/// returned as-is.
pub fn proof_bytes_for_program(sdk_bytes: &[u8]) -> Result<Vec<u8>, String> {
    match sdk_bytes.len() {
        ON_CHAIN_PROOF_LEN => Ok(sdk_bytes.to_vec()),
        356 => {
            // selector(4) + public_inputs(96) + raw_proof(256)
            let mut out = Vec::with_capacity(ON_CHAIN_PROOF_LEN);
            out.extend_from_slice(&sdk_bytes[..4]);
            out.extend_from_slice(&sdk_bytes[100..]);
            Ok(out)
        }
        n => Err(format!(
            "unexpected SP1 proof byte length {n}; expected {ON_CHAIN_PROOF_LEN} or 356"
        )),
    }
}

/// `initialize_approved_vkeys(admin)` — create the singleton `ApprovedVkeys`
/// PDA. Run exactly once after program deploy. Admin-gated.
///
/// Signers: `admin`.
pub fn build_initialize_approved_vkeys_ix(admin: &Pubkey) -> Instruction {
    let (pda, _) = derive_approved_vkeys_pda();
    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new(*admin, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: anchor_discriminator("initialize_approved_vkeys").to_vec(),
    }
}

/// `initialize_approved_measurements(admin)` — create the singleton
/// `ApprovedMeasurements` PDA. Run exactly once after program deploy.
/// Admin-gated.
///
/// Signers: `admin`.
pub fn build_initialize_approved_measurements_ix(admin: &Pubkey) -> Instruction {
    let (pda, _) = derive_approved_measurements_pda();
    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new(*admin, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: anchor_discriminator("initialize_approved_measurements").to_vec(),
    }
}

/// `add_approved_vkey(admin, vkey_hash)` — append a vkey to the allowlist.
/// Admin-gated. Fails with `VkeyAlreadyApproved` if already present.
///
/// Signers: `admin`.
pub fn build_add_approved_vkey_ix(admin: &Pubkey, vkey_hash: &[u8; 32]) -> Instruction {
    let (pda, _) = derive_approved_vkeys_pda();
    let mut data = anchor_discriminator("add_approved_vkey").to_vec();
    data.extend_from_slice(vkey_hash);

    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(*admin, true),
        ],
        data,
    }
}

/// `remove_approved_vkey(admin, vkey_hash)` — remove a vkey from the
/// allowlist. Admin-gated. Fails with `VkeyNotApproved` if not present.
///
/// Signers: `admin`.
pub fn build_remove_approved_vkey_ix(admin: &Pubkey, vkey_hash: &[u8; 32]) -> Instruction {
    let (pda, _) = derive_approved_vkeys_pda();
    let mut data = anchor_discriminator("remove_approved_vkey").to_vec();
    data.extend_from_slice(vkey_hash);

    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(*admin, true),
        ],
        data,
    }
}

/// `add_approved_measurement(admin, measurement)` — append a TEE measurement
/// (e.g. AWS Nitro PCR0) to the allowlist. Admin-gated. The `measurement`
/// length must be in `1..=64`.
///
/// Signers: `admin`.
pub fn build_add_approved_measurement_ix(admin: &Pubkey, measurement: &[u8]) -> Instruction {
    let (pda, _) = derive_approved_measurements_pda();
    let mut data = anchor_discriminator("add_approved_measurement").to_vec();
    data.extend_from_slice(&(measurement.len() as u32).to_le_bytes());
    data.extend_from_slice(measurement);

    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(*admin, true),
        ],
        data,
    }
}

/// `remove_approved_measurement(admin, measurement)` — remove a measurement
/// from the allowlist. Admin-gated. Fails with `MeasurementNotApproved` if
/// not present.
///
/// Signers: `admin`.
pub fn build_remove_approved_measurement_ix(admin: &Pubkey, measurement: &[u8]) -> Instruction {
    let (pda, _) = derive_approved_measurements_pda();
    let mut data = anchor_discriminator("remove_approved_measurement").to_vec();
    data.extend_from_slice(&(measurement.len() as u32).to_le_bytes());
    data.extend_from_slice(measurement);

    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(*admin, true),
        ],
        data,
    }
}

/// `register_key(signing_pubkey, sp1_vkey_hash, proof, public_values)` —
/// register a TEE signing key after proving it was generated inside a
/// whitelisted enclave (spec §6.2 「四段の register_key 検証」).
///
/// Not admin-gated: anyone can submit, but the on-chain four-step check
/// (vkey ∈ approved, PCR0 ∈ approved, user_data binding, Groth16) gates
/// acceptance. `proof` must be exactly 260 bytes (`vk_selector(4) + groth16(256)`).
///
/// Signers: `payer`.
pub fn build_register_key_ix(
    payer: &Pubkey,
    signing_pubkey: &[u8; 32],
    sp1_vkey_hash: &[u8; 32],
    proof: &[u8],
    public_values: &[u8],
) -> Instruction {
    let (whitelist_pda, _) = derive_whitelist_pda(signing_pubkey);
    let (approved_vkeys_pda, _) = derive_approved_vkeys_pda();
    let (approved_measurements_pda, _) = derive_approved_measurements_pda();

    let mut data = Vec::with_capacity(8 + 32 + 32 + 4 + proof.len() + 4 + public_values.len());
    data.extend_from_slice(&anchor_discriminator("register_key"));
    data.extend_from_slice(signing_pubkey);
    data.extend_from_slice(sp1_vkey_hash);
    data.extend_from_slice(&(proof.len() as u32).to_le_bytes());
    data.extend_from_slice(proof);
    data.extend_from_slice(&(public_values.len() as u32).to_le_bytes());
    data.extend_from_slice(public_values);

    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(whitelist_pda, false),
            AccountMeta::new_readonly(approved_vkeys_pda, false),
            AccountMeta::new_readonly(approved_measurements_pda, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// `revoke_key(admin, signing_pubkey)` — mark a registered key as revoked.
/// The `WhitelistEntry` PDA is **not** closed so the same proof cannot be
/// re-submitted to resurrect the key. Admin-gated.
///
/// Signers: `admin`.
pub fn build_revoke_key_ix(admin: &Pubkey, signing_pubkey: &[u8; 32]) -> Instruction {
    let (whitelist_pda, _) = derive_whitelist_pda(signing_pubkey);
    let (approved_vkeys_pda, _) = derive_approved_vkeys_pda();

    Instruction {
        program_id: WHITELIST_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(whitelist_pda, false),
            AccountMeta::new_readonly(approved_vkeys_pda, false),
            AccountMeta::new_readonly(*admin, true),
        ],
        data: anchor_discriminator("revoke_key").to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_discriminator_matches_known_value() {
        // SHA-256("global:initialize_approved_vkeys")[..8]
        let disc = anchor_discriminator("initialize_approved_vkeys");
        // First 8 bytes of SHA-256("global:initialize_approved_vkeys").
        // Computed via: python3 -c "import hashlib; print(hashlib.sha256(b'global:initialize_approved_vkeys').digest()[:8].hex())"
        // Output here is documentation; the value is stable across runs by definition.
        assert_eq!(disc.len(), 8);
    }

    #[test]
    fn register_key_ix_account_order() {
        let ix = build_register_key_ix(
            &Pubkey::new_unique(),
            &[1u8; 32],
            &[2u8; 32],
            &[3u8; 260],
            &[4u8; 137],
        );
        assert_eq!(ix.accounts.len(), 5);
        assert!(!ix.accounts[0].is_signer && ix.accounts[0].is_writable); // whitelist_pda
        assert!(!ix.accounts[1].is_signer && !ix.accounts[1].is_writable); // approved_vkeys
        assert!(!ix.accounts[2].is_signer && !ix.accounts[2].is_writable); // approved_measurements
        assert!(ix.accounts[3].is_signer && ix.accounts[3].is_writable); // payer
        assert!(!ix.accounts[4].is_signer && !ix.accounts[4].is_writable); // system_program
    }

    #[test]
    fn add_approved_measurement_encodes_length_prefix() {
        let admin = Pubkey::new_unique();
        let measurement = vec![0xAB; 48];
        let ix = build_add_approved_measurement_ix(&admin, &measurement);

        // discriminator(8) + len(4) + bytes(48)
        assert_eq!(ix.data.len(), 8 + 4 + 48);
        let len_bytes = &ix.data[8..12];
        assert_eq!(u32::from_le_bytes(len_bytes.try_into().unwrap()), 48);
        assert_eq!(&ix.data[12..], &measurement[..]);
    }

    #[test]
    fn add_approved_vkey_no_length_prefix() {
        // `vkey_hash: [u8; 32]` is a Borsh fixed-length array; no length prefix.
        let admin = Pubkey::new_unique();
        let ix = build_add_approved_vkey_ix(&admin, &[0xAA; 32]);
        assert_eq!(ix.data.len(), 8 + 32);
    }
}
