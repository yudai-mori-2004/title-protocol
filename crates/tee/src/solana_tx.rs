//! # Solanaトランザクション構築ヘルパー
//!
//! 仕様書 §5.1, §6.4
//!
//! Bubblegum V2 (cNFT) 関連のトランザクション構築を行う。
//! mpl-bubblegumクレートのビルダーを使用する。

use mpl_bubblegum::instructions::{CreateTreeConfigV2Builder, MintV2Builder};
use mpl_bubblegum::types::{Creator, MetadataArgsV2, TokenStandard};
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    signature::Signature,
    transaction::Transaction,
};
use std::str::FromStr;

// ---------------------------------------------------------------------------
// プログラムID (V2)
// ---------------------------------------------------------------------------

/// SPL Account Compression V2 プログラムID。
/// Bubblegum V2が使用する新しいアドレス。
pub fn spl_account_compression_v2_id() -> Pubkey {
    Pubkey::from_str("mcmt6YrQEMKw8Mw43FmpRLmf7BqRnFMKmAcbxE3xkAW").unwrap()
}

// ---------------------------------------------------------------------------
// PDA導出
// ---------------------------------------------------------------------------

/// Bubblegum tree_config PDAを導出する。
/// 仕様書 §6.4 Step 2
/// seeds = [merkle_tree.key()], program = Bubblegum
pub fn derive_tree_config(merkle_tree: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[merkle_tree.as_ref()], &mpl_bubblegum::ID)
}

/// MPL Core CPI Signer PDAを導出する。
/// MintV2でコレクション付きミント時に必要。
/// seeds = [b"mpl_core_cpi_signer"], program = Bubblegum
pub fn derive_mpl_core_cpi_signer() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"mpl_core_cpi_signer"], &mpl_bubblegum::ID)
}

// ---------------------------------------------------------------------------
// Merkle Tree サイズ計算
// ---------------------------------------------------------------------------

/// Merkle Treeアカウントに必要なデータサイズを計算する。
/// spl-account-compressionのConcurrentMerkleTreeレイアウトに基づく。
pub fn merkle_tree_account_size(max_depth: u32, max_buffer_size: u32) -> usize {
    let d = max_depth as usize;
    let b = max_buffer_size as usize;

    // ヘッダー: discriminator(8) + CompressionAccountType(1) + max_buffer_size(4) +
    //   max_depth(4) + authority(32) + creation_slot(8) + is_batch_initialized(1) + padding(5)
    let header_size = 8 + 1 + 4 + 4 + 32 + 8 + 1 + 5; // = 63

    // ConcurrentMerkleTree: sequence_number(8) + active_index(8) + buffer_size(8)
    let tree_header = 24;

    // ChangeLog: root(32) + path_nodes(d * 32) + index(4) + _padding(4)
    let change_log_size = 32 + d * 32 + 4 + 4;

    // RightMostPath: leaf(32) + proof(d * 32) + index(4)
    let path_size = 32 + d * 32 + 4;

    header_size + tree_header + b * change_log_size + path_size
}

/// Solanaのrent-exempt minimum lamportsを計算する。
/// `(128 + data_len) * 6960`
pub fn rent_exempt_minimum(data_len: usize) -> u64 {
    (128 + data_len as u64) * 6960
}

// ---------------------------------------------------------------------------
// create_tree V2 トランザクション構築
// ---------------------------------------------------------------------------

/// Bubblegum V2 CreateTreeConfig トランザクションを構築する。
/// 仕様書 §6.4 Step 2, §6.5 Merkle Tree
///
/// トランザクションには2つの命令が含まれる:
/// 1. system_program::create_account — Merkle Treeアカウントの割り当て
/// 2. bubblegum::create_tree_config_v2 — TreeConfigの初期化
///
/// 署名者: payer (fee payer), tree_pubkey (new account), tree_creator (TEE署名鍵)
/// TEEはtree_pubkeyとtree_creatorで部分署名する。payerは後から署名を追加する。
pub fn build_create_tree_tx(
    payer: &Pubkey,
    tree_pubkey: &Pubkey,
    tree_creator: &Pubkey,
    max_depth: u32,
    max_buffer_size: u32,
    blockhash: &solana_sdk::hash::Hash,
) -> Transaction {
    let space = merkle_tree_account_size(max_depth, max_buffer_size);
    let lamports = rent_exempt_minimum(space);

    // 命令1: Merkle Treeアカウントの作成
    let create_account_ix = solana_sdk::system_instruction::create_account(
        payer,
        tree_pubkey,
        lamports,
        space as u64,
        &spl_account_compression_v2_id(),
    );

    // 命令2: Bubblegum V2 CreateTreeConfig
    let (tree_config, _) = derive_tree_config(tree_pubkey);

    let create_tree_ix = CreateTreeConfigV2Builder::new()
        .tree_config(tree_config)
        .merkle_tree(*tree_pubkey)
        .payer(*payer)
        .tree_creator(Some(*tree_creator))
        .max_depth(max_depth)
        .max_buffer_size(max_buffer_size)
        .instruction();

    let message = Message::new_with_blockhash(
        &[create_account_ix, create_tree_ix],
        Some(payer),
        blockhash,
    );

    let num_signers = message.header.num_required_signatures as usize;
    let signatures = vec![Signature::default(); num_signers];

    Transaction {
        signatures,
        message,
    }
}

// ---------------------------------------------------------------------------
// mint V2 トランザクション構築
// ---------------------------------------------------------------------------

/// Bubblegum V2 MintV2 トランザクションを構築する。
/// 仕様書 §5.1 Step 9-10, §6.5 Merkle Tree
///
/// core_collectionが指定された場合、MPL-Coreコレクションへのミントを行う。
/// TEEがcollection_authorityとtree_creator_or_delegateの両方を兼ねる。
///
/// 署名者: creator_wallet (fee payer), tee_signing_pubkey (tree delegate + collection authority)
/// TEEはtee_signing_pubkeyで部分署名する。creator_walletは後から署名を追加する。
pub fn build_mint_v2_tx(
    tree_pubkey: &Pubkey,
    tee_signing_pubkey: &Pubkey,
    creator_wallet: &Pubkey,
    content_hash: &str,
    signed_json_uri: &str,
    core_collection: Option<&Pubkey>,
    blockhash: &solana_sdk::hash::Hash,
) -> Transaction {
    let (tree_config, _) = derive_tree_config(tree_pubkey);

    // cNFTメタデータ構築（仕様書 §5.1 Step 11）
    let hash_suffix = if content_hash.len() > 2 {
        &content_hash[2..content_hash.len().min(10)]
    } else {
        content_hash
    };
    let name = format!("Title #{hash_suffix}");

    let metadata = MetadataArgsV2 {
        name,
        symbol: "TITLE".to_string(),
        uri: signed_json_uri.to_string(),
        seller_fee_basis_points: 0,
        primary_sale_happened: false,
        is_mutable: false,
        token_standard: Some(TokenStandard::NonFungible),
        creators: vec![Creator {
            address: *creator_wallet,
            verified: false,
            share: 100,
        }],
        collection: core_collection.copied(),
    };

    let mut builder = MintV2Builder::new();
    builder
        .tree_config(tree_config)
        .payer(*creator_wallet)
        .tree_creator_or_delegate(Some(*tee_signing_pubkey))
        .leaf_owner(*creator_wallet)
        .merkle_tree(*tree_pubkey)
        .metadata(metadata);

    // コレクション付きミント（仕様書 §5.1 Step 11）
    if let Some(collection) = core_collection {
        let (mpl_core_cpi_signer, _) = derive_mpl_core_cpi_signer();
        builder
            .core_collection(Some(*collection))
            .collection_authority(Some(*tee_signing_pubkey))
            .mpl_core_cpi_signer(Some(mpl_core_cpi_signer));
    }

    let mint_ix = builder.instruction();

    let message = Message::new_with_blockhash(
        &[mint_ix],
        Some(creator_wallet),
        blockhash,
    );

    let num_signers = message.header.num_required_signatures as usize;
    let signatures = vec![Signature::default(); num_signers];

    Transaction {
        signatures,
        message,
    }
}

// ---------------------------------------------------------------------------
// 部分署名ヘルパー
// ---------------------------------------------------------------------------

/// トランザクションに部分署名を適用する。
/// 指定した公開鍵に対応する署名スロットにEd25519署名をセットする。
pub fn apply_partial_signature(
    tx: &mut Transaction,
    pubkey: &Pubkey,
    signature_bytes: &[u8],
) -> Result<(), String> {
    let sig_arr: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| "署名は64バイトである必要があります".to_string())?;
    let signature = Signature::from(sig_arr);

    let num_signers = tx.message.header.num_required_signatures as usize;
    for (i, key) in tx.message.account_keys.iter().enumerate() {
        if i >= num_signers {
            break;
        }
        if key == pubkey {
            tx.signatures[i] = signature;
            return Ok(());
        }
    }

    Err(format!(
        "公開鍵 {} がトランザクションの署名者に見つかりません",
        pubkey
    ))
}

/// トランザクションをバイナリにシリアライズする。
pub fn serialize_transaction(tx: &Transaction) -> Result<Vec<u8>, String> {
    bincode::serialize(tx).map_err(|e| format!("トランザクションのシリアライズに失敗: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_tree_account_size() {
        // max_depth=20, max_buffer_size=64 の一般的な構成
        let size = merkle_tree_account_size(20, 64);
        assert!(size > 0);
        // 既知の値に近いことを確認（約44KB）
        assert!(size > 40_000 && size < 50_000, "size={size}");
    }

    #[test]
    fn test_derive_tree_config() {
        let tree = Pubkey::new_unique();
        let (config, bump) = derive_tree_config(&tree);
        assert_ne!(config, tree);
        let _ = bump;
        // 決定論的であることを確認
        let (config2, bump2) = derive_tree_config(&tree);
        assert_eq!(config, config2);
        assert_eq!(bump, bump2);
    }

    #[test]
    fn test_derive_mpl_core_cpi_signer() {
        let (signer, bump) = derive_mpl_core_cpi_signer();
        let _ = bump;
        // 決定論的であることを確認
        let (signer2, _) = derive_mpl_core_cpi_signer();
        assert_eq!(signer, signer2);
    }

    #[test]
    fn test_build_create_tree_tx() {
        let payer = Pubkey::new_unique();
        let tree = Pubkey::new_unique();
        let tree_creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();

        let tx = build_create_tree_tx(&payer, &tree, &tree_creator, 20, 64, &blockhash);

        // 3つの署名者（payer, tree, tree_creator）
        assert_eq!(tx.message.header.num_required_signatures, 3);
        // 2つの命令（create_account + create_tree_config_v2）
        assert_eq!(tx.message.instructions.len(), 2);
    }

    #[test]
    fn test_build_mint_v2_tx_without_collection() {
        let tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();

        let tx = build_mint_v2_tx(
            &tree,
            &tee_signer,
            &creator,
            "0x1234abcdef567890",
            "ar://test_uri",
            None,
            &blockhash,
        );

        // 2つの署名者（creator/payer, tee_signer）
        assert_eq!(tx.message.header.num_required_signatures, 2);
        // 1つの命令（mint_v2）
        assert_eq!(tx.message.instructions.len(), 1);
    }

    #[test]
    fn test_build_mint_v2_tx_with_collection() {
        let tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let collection = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();

        let tx = build_mint_v2_tx(
            &tree,
            &tee_signer,
            &creator,
            "0x1234abcdef567890",
            "ar://test_uri",
            Some(&collection),
            &blockhash,
        );

        // 2つの署名者（creator/payer, tee_signer）
        // tee_signerはtree_creator_or_delegateとcollection_authorityを兼ねるため重複排除
        assert_eq!(tx.message.header.num_required_signatures, 2);
        // 1つの命令（mint_v2）
        assert_eq!(tx.message.instructions.len(), 1);
    }

    #[test]
    fn test_apply_partial_signature() {
        let payer = Pubkey::new_unique();
        let tree = Pubkey::new_unique();
        let tree_creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();

        let mut tx = build_create_tree_tx(&payer, &tree, &tree_creator, 20, 64, &blockhash);

        // 64バイトのダミー署名
        let dummy_sig = [1u8; 64];
        let result = apply_partial_signature(&mut tx, &tree_creator, &dummy_sig);
        assert!(result.is_ok());

        // 存在しない公開鍵では失敗
        let unknown = Pubkey::new_unique();
        let result = apply_partial_signature(&mut tx, &unknown, &dummy_sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_transaction() {
        let payer = Pubkey::new_unique();
        let tree = Pubkey::new_unique();
        let tree_creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();

        let tx = build_create_tree_tx(&payer, &tree, &tree_creator, 20, 64, &blockhash);
        let bytes = serialize_transaction(&tx);
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }
}
