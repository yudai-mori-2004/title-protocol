// SPDX-License-Identifier: Apache-2.0

//! # cNFT Transaction Construction
//!
//! Spec §6.2 — Bubblegum V2 cNFT mint transaction construction and partial signing.

use crate::signing_key::SolanaSigningKey;
use mpl_bubblegum::instructions::{CreateTreeConfigV2Builder, MintV2Builder};
use mpl_bubblegum::types::{Creator, MetadataArgsV2, TokenStandard};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    message::{self, AddressLookupTableAccount, VersionedMessage},
    pubkey,
    pubkey::Pubkey,
    signature::Signature,
    system_instruction,
    transaction::VersionedTransaction,
};

/// SPL Account Compression V2 program ID — backs Bubblegum V2 Merkle trees.
pub const SPL_ACCOUNT_COMPRESSION_V2_ID: Pubkey =
    pubkey!("mcmt6YrQEMKw8Mw43FmpRLmf7BqRnFMKmAcbxE3xkAW");

/// `CreateTreeConfigV2` + `system_program::create_account` の実測 CU は
/// ~150K。tree depth / buffer 変動への余裕を見て 400K に設定。
const CU_LIMIT_CREATE_TREE: u32 = 400_000;

/// MintV2 + MPL Core CPI (with `core_collection`) の実測 CU は ~280K。
/// MPL Core ハンドラが支配的で余裕を見て 400K に設定。
const CU_LIMIT_MINT_V2_WITH_COLLECTION: u32 = 400_000;

/// MintV2 (without `core_collection`、Bubblegum 単独経路) の実測 CU は
/// ~150K。ALT lookup の変動余裕で 250K に設定。
const CU_LIMIT_MINT_V2_NO_COLLECTION: u32 = 250_000;

/// Derive Bubblegum tree_config PDA. Seeds: `[merkle_tree]`.
pub fn derive_tree_config(merkle_tree: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[merkle_tree.as_ref()], &mpl_bubblegum::ID)
}

/// Derive the MPL Core CPI Signer PDA used by Bubblegum V2 when minting
/// into an MPL Core collection. Seeds: `[b"mpl_core_cpi_signer"]`,
/// program = Bubblegum. The seed is defined inside the Bubblegum program
/// (not re-exported); keep this in sync if Bubblegum changes the convention.
///
/// Singleton なので `OnceLock` で 1 回だけ `find_program_address` を回す。
pub(crate) fn derive_mpl_core_cpi_signer() -> (Pubkey, u8) {
    static CACHE: std::sync::OnceLock<(Pubkey, u8)> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        Pubkey::find_program_address(&[b"mpl_core_cpi_signer"], &mpl_bubblegum::ID)
    })
}

/// Calculate the data size for a ConcurrentMerkleTree account.
/// Matches spl-account-compression V2 layout.
pub fn merkle_tree_account_size(max_depth: u32, max_buffer_size: u32) -> usize {
    let d = max_depth as usize;
    let b = max_buffer_size as usize;

    // Account header: CompressionAccountType(1) + HeaderVersion(1) +
    //   max_buffer_size(4) + max_depth(4) + authority(32) + creation_slot(8) + padding(6)
    let header_size = 56;

    // ConcurrentMerkleTree: sequence_number(8) + active_index(8) + buffer_size(8)
    let tree_header = 24;

    // ChangeLog: root(32) + path_nodes(d * 32) + index(4) + _padding(4)
    let change_log_size = 32 + d * 32 + 4 + 4;

    // RightMostPath: proof(d * 32) + leaf(32) + index(4) + _padding(4)
    let path_size = d * 32 + 32 + 4 + 4;

    header_size + tree_header + b * change_log_size + path_size
}

/// Compute Solana rent-exempt minimum lamports.
pub fn rent_exempt_minimum(data_len: usize) -> u64 {
    (128 + data_len as u64) * 6960
}

/// Build a Bubblegum V2 CreateTreeConfig transaction.
/// Spec §6.2 — Merkle tree creation for cNFT storage.
///
/// Three instructions:
/// 1. ComputeBudget — raise CU limit
/// 2. system_program::create_account — allocate Merkle tree account
/// 3. bubblegum::create_tree_config_v2 — initialize TreeConfig PDA
///
/// Signers: payer (fee payer), tree_keypair (new account), tree_creator (TEE key)
pub fn build_create_tree_tx(
    payer: &Pubkey,
    tree_pubkey: &Pubkey,
    tree_creator: &Pubkey,
    max_depth: u32,
    max_buffer_size: u32,
    blockhash: &Hash,
) -> Result<VersionedTransaction, CnftError> {
    let space = merkle_tree_account_size(max_depth, max_buffer_size);
    let lamports = rent_exempt_minimum(space);

    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT_CREATE_TREE);

    let create_account_ix = system_instruction::create_account(
        payer,
        tree_pubkey,
        lamports,
        space as u64,
        &SPL_ACCOUNT_COMPRESSION_V2_ID,
    );

    let (tree_config, _) = derive_tree_config(tree_pubkey);

    let create_tree_ix = CreateTreeConfigV2Builder::new()
        .tree_config(tree_config)
        .merkle_tree(*tree_pubkey)
        .payer(*payer)
        .tree_creator(Some(*tree_creator))
        .max_depth(max_depth)
        .max_buffer_size(max_buffer_size)
        .instruction();

    build_v0_tx(
        &[compute_budget_ix, create_account_ix, create_tree_ix],
        payer,
        blockhash,
        &[],
    )
}

/// Errors from cNFT transaction construction.
#[derive(Debug, thiserror::Error)]
pub enum CnftError {
    #[error("Failed to compile V0 message: {0}")]
    MessageCompileFailed(String),

    #[error("Transaction serialization failed: {0}")]
    SerializeFailed(String),

    #[error("Signing failed: {0}")]
    SigningFailed(#[from] crate::signing_key::SigningKeyError),
}

/// Build a VersionedTransaction (V0) from instructions.
pub fn build_v0_tx(
    instructions: &[solana_sdk::instruction::Instruction],
    payer: &Pubkey,
    blockhash: &Hash,
    alt_accounts: &[AddressLookupTableAccount],
) -> Result<VersionedTransaction, CnftError> {
    let msg_v0 = message::v0::Message::try_compile(payer, instructions, alt_accounts, *blockhash)
        .map_err(|e| CnftError::MessageCompileFailed(e.to_string()))?;
    let versioned_msg = VersionedMessage::V0(msg_v0);
    let num_signers = versioned_msg.header().num_required_signatures as usize;
    let sigs = vec![Signature::default(); num_signers];
    Ok(VersionedTransaction {
        signatures: sigs,
        message: versioned_msg,
    })
}

/// Build a Bubblegum V2 MintV2 instruction.
/// Spec §6.2 — cNFT mint with TEE signing key as tree_creator_or_delegate.
pub fn build_mint_v2_ix(
    tree_pubkey: &Pubkey,
    tee_signing_pubkey: &Pubkey,
    leaf_owner: &Pubkey,
    signature_hash: &str,
    offchain_uri: &str,
    core_collection: Option<&Pubkey>,
    payer: &Pubkey,
) -> solana_sdk::instruction::Instruction {
    let (tree_config, _) = derive_tree_config(tree_pubkey);

    // signature_hash is `sha256:<hex>` (Spec §1.5); take a short slice of
    // the hex tail for a human-readable display name. `strip_prefix` is
    // safe for either form; `min(8)` caps the suffix length.
    let hex = signature_hash
        .strip_prefix("sha256:")
        .unwrap_or(signature_hash);
    let short = &hex[..hex.len().min(8)];
    let name = format!("Title #{short}");

    let metadata = MetadataArgsV2 {
        name,
        symbol: "TITLE".to_string(),
        uri: offchain_uri.to_string(),
        seller_fee_basis_points: 0,
        primary_sale_happened: false,
        is_mutable: false,
        token_standard: Some(TokenStandard::NonFungible),
        creators: vec![Creator {
            address: *leaf_owner,
            verified: false,
            share: 100,
        }],
        collection: core_collection.copied(),
    };

    let mut builder = MintV2Builder::new();
    builder
        .tree_config(tree_config)
        .payer(*payer)
        .tree_creator_or_delegate(Some(*tee_signing_pubkey))
        .leaf_owner(*leaf_owner)
        .merkle_tree(*tree_pubkey)
        .metadata(metadata);

    if let Some(collection) = core_collection {
        let (mpl_core_cpi_signer, _) = derive_mpl_core_cpi_signer();
        builder
            .core_collection(Some(*collection))
            .collection_authority(Some(*tee_signing_pubkey))
            .mpl_core_cpi_signer(Some(mpl_core_cpi_signer));
    }

    builder.instruction()
}

/// Build a cNFT mint transaction and partially sign it with the TEE signing key.
/// Spec §6.2 — cNFT発行トランザクション構築 + TEE署名鍵で部分署名
///
/// Returns the partially signed transaction. The caller must add the final
/// signature (payer/leaf_owner) and broadcast.
pub fn build_and_sign_mint_tx(
    signing_key: &SolanaSigningKey,
    tree_pubkey: &Pubkey,
    leaf_owner: &Pubkey,
    signature_hash: &str,
    offchain_uri: &str,
    core_collection: Option<&Pubkey>,
    payer: &Pubkey,
    recent_blockhash: &Hash,
) -> Result<VersionedTransaction, CnftError> {
    let tee_pubkey = signing_key.pubkey();

    let ix = build_mint_v2_ix(
        tree_pubkey,
        &tee_pubkey,
        leaf_owner,
        signature_hash,
        offchain_uri,
        core_collection,
        payer,
    );

    // MintV2 + MPL Core CPI runs over the 200K default; reserve enough
    // headroom for the collection path. cNFT mints without a collection
    // would survive on the default but a single budget keeps both shapes
    // uniformly fast.
    let cu_limit = if core_collection.is_some() {
        CU_LIMIT_MINT_V2_WITH_COLLECTION
    } else {
        CU_LIMIT_MINT_V2_NO_COLLECTION
    };
    let cu_ix = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);

    let mut tx = build_v0_tx(&[cu_ix, ix], payer, recent_blockhash, &[])?;
    signing_key.sign_transaction(&mut tx)?;

    Ok(tx)
}

/// Serialize a VersionedTransaction to bytes.
pub fn serialize_transaction(tx: &VersionedTransaction) -> Result<Vec<u8>, CnftError> {
    bincode::serialize(tx).map_err(|e| CnftError::SerializeFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_tree_config_deterministic() {
        let tree = Pubkey::new_unique();
        let (c1, b1) = derive_tree_config(&tree);
        let (c2, b2) = derive_tree_config(&tree);
        assert_eq!(c1, c2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn derive_mpl_core_cpi_signer_deterministic() {
        let (s1, _) = derive_mpl_core_cpi_signer();
        let (s2, _) = derive_mpl_core_cpi_signer();
        assert_eq!(s1, s2);
    }

    #[test]
    fn build_mint_v2_ix_no_collection() {
        let tree = Pubkey::new_unique();
        let tee = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let ix = build_mint_v2_ix(
            &tree,
            &tee,
            &owner,
            "sha256:abcdef12",
            "https://example.com/data.json",
            None,
            &owner,
        );

        assert_eq!(ix.program_id, mpl_bubblegum::ID);
        assert!(!ix.data.is_empty());
    }

    #[test]
    fn build_mint_v2_ix_with_collection() {
        let tree = Pubkey::new_unique();
        let tee = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let collection = Pubkey::new_unique();

        let ix = build_mint_v2_ix(
            &tree,
            &tee,
            &owner,
            "sha256:abcdef12",
            "https://example.com/data.json",
            Some(&collection),
            &owner,
        );

        assert_eq!(ix.program_id, mpl_bubblegum::ID);
        assert!(!ix.data.is_empty());
        assert!(!ix.accounts.is_empty());
    }

    #[test]
    fn build_v0_tx_basic() {
        let tree = Pubkey::new_unique();
        let tee = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let blockhash = Hash::new_unique();

        let ix = build_mint_v2_ix(
            &tree,
            &tee,
            &owner,
            "sha256:abcdef12",
            "https://example.com/data.json",
            None,
            &owner,
        );

        let tx = build_v0_tx(&[ix], &owner, &blockhash, &[]).unwrap();
        let bytes = serialize_transaction(&tx).unwrap();
        assert!(!bytes.is_empty());
        assert!(bytes.len() <= 1232);
    }

    #[test]
    fn build_and_sign_mint_tx_applies_signature() {
        let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
        let tree = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let blockhash = Hash::new_unique();

        let tx = build_and_sign_mint_tx(
            &key,
            &tree,
            &owner,
            "sha256:abcdef1234567890",
            "https://example.com/data.json",
            None,
            &owner,
            &blockhash,
        )
        .unwrap();

        // TEE signing key's signature slot should not be default
        let tee_pubkey = key.pubkey();
        let static_keys = tx.message.static_account_keys();
        let num_signers = tx.message.header().num_required_signatures as usize;

        let mut found = false;
        for i in 0..num_signers {
            if i < static_keys.len() && static_keys[i] == tee_pubkey {
                assert_ne!(tx.signatures[i], Signature::default());
                found = true;
            }
        }
        assert!(found, "TEE signing key should be in the signers list");
    }

    #[test]
    fn serialize_transaction_roundtrip() {
        let tree = Pubkey::new_unique();
        let tee = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let blockhash = Hash::new_unique();

        let ix = build_mint_v2_ix(
            &tree,
            &tee,
            &owner,
            "sha256:abcdef12",
            "https://example.com/data.json",
            None,
            &owner,
        );

        let tx = build_v0_tx(&[ix], &owner, &blockhash, &[]).unwrap();
        let bytes = serialize_transaction(&tx).unwrap();

        let restored: VersionedTransaction = bincode::deserialize(&bytes).unwrap();
        assert_eq!(tx.signatures.len(), restored.signatures.len());
    }
}
