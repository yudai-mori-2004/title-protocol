// SPDX-License-Identifier: Apache-2.0

//! # Solanaトランザクション構築ヘルパー
//!
//! 仕様書 §5.1, §6.4
//!
//! Bubblegum V2 (cNFT) 関連のトランザクション構築を行う。
//! 全トランザクションは VersionedTransaction (v0) で統一。
//! MintV2 では ALT によるアカウント圧縮を適用する。

use mpl_bubblegum::instructions::{CreateTreeConfigV2Builder, MintV2Builder};
use mpl_bubblegum::types::{Creator, MetadataArgsV2, TokenStandard};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    message::{self, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    transaction::VersionedTransaction,
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
/// spl-account-compression (V2) のConcurrentMerkleTreeレイアウトに基づく。
/// @solana/spl-account-compression の getConcurrentMerkleTreeAccountSize と一致する。
pub fn merkle_tree_account_size(max_depth: u32, max_buffer_size: u32) -> usize {
    let d = max_depth as usize;
    let b = max_buffer_size as usize;

    // アカウントヘッダー: CompressionAccountType(1) + HeaderVersion(1) +
    //   max_buffer_size(4) + max_depth(4) + authority(32) + creation_slot(8) + padding(6)
    let header_size = 1 + 1 + 4 + 4 + 32 + 8 + 6; // = 56

    // ConcurrentMerkleTree: sequence_number(8) + active_index(8) + buffer_size(8)
    let tree_header = 24;

    // ChangeLog: root(32) + path_nodes(d * 32) + index(4) + _padding(4)
    let change_log_size = 32 + d * 32 + 4 + 4;

    // RightMostPath: proof(d * 32) + leaf(32) + index(4) + _padding(4)
    let path_size = d * 32 + 32 + 4 + 4;

    header_size + tree_header + b * change_log_size + path_size
}

/// Solanaのrent-exempt minimum lamportsを計算する。
/// `(128 + data_len) * 6960`
pub fn rent_exempt_minimum(data_len: usize) -> u64 {
    (128 + data_len as u64) * 6960
}

// ---------------------------------------------------------------------------
// VersionedTransaction 構築ヘルパー
// ---------------------------------------------------------------------------

/// instruction群から VersionedTransaction (v0) を構築する。
/// ALT が不要な場合は空スライスを渡す。
pub fn build_v0_tx(
    instructions: &[solana_sdk::instruction::Instruction],
    payer: &Pubkey,
    blockhash: &solana_sdk::hash::Hash,
    alt_accounts: &[AddressLookupTableAccount],
) -> Result<VersionedTransaction, String> {
    let msg_v0 = message::v0::Message::try_compile(payer, instructions, alt_accounts, *blockhash)
        .map_err(|e| format!("MessageV0のコンパイルに失敗: {e}"))?;
    let versioned_msg = VersionedMessage::V0(msg_v0);
    let num_signers = versioned_msg.header().num_required_signatures as usize;
    let sigs = vec![Signature::default(); num_signers];
    Ok(VersionedTransaction {
        signatures: sigs,
        message: versioned_msg,
    })
}

// ---------------------------------------------------------------------------
// create_tree V2 トランザクション構築
// ---------------------------------------------------------------------------

/// Bubblegum V2 CreateTreeConfig トランザクションを構築する。
/// 仕様書 §6.4 Step 2, §6.5 Merkle Tree
///
/// トランザクションには3つの命令が含まれる:
/// 1. ComputeBudget — CU上限の引き上げ
/// 2. system_program::create_account — Merkle Treeアカウントの割り当て
/// 3. bubblegum::create_tree_config_v2 — TreeConfigの初期化
///
/// 署名者: payer (fee payer), tree_pubkey (new account), tree_creator (TEE署名鍵)
pub fn build_create_tree_tx(
    payer: &Pubkey,
    tree_pubkey: &Pubkey,
    tree_creator: &Pubkey,
    max_depth: u32,
    max_buffer_size: u32,
    blockhash: &solana_sdk::hash::Hash,
) -> VersionedTransaction {
    let space = merkle_tree_account_size(max_depth, max_buffer_size);
    let lamports = rent_exempt_minimum(space);

    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);

    let create_account_ix = solana_sdk::system_instruction::create_account(
        payer,
        tree_pubkey,
        lamports,
        space as u64,
        &spl_account_compression_v2_id(),
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

    // create-tree は ALT 不要
    build_v0_tx(
        &[compute_budget_ix, create_account_ix, create_tree_ix],
        payer,
        blockhash,
        &[],
    )
    .expect("create_tree_txの構築に失敗")
}

// ---------------------------------------------------------------------------
// mint V2 トランザクション構築
// ---------------------------------------------------------------------------

/// Bubblegum V2 MintV2 Instruction を1つ生成する。
/// 仕様書 §5.1 Step 9-10, §6.5
pub fn build_mint_v2_ix(
    tree_pubkey: &Pubkey,
    tee_signing_pubkey: &Pubkey,
    creator_wallet: &Pubkey,
    content_hash: &str,
    signed_json_uri: &str,
    core_collection: Option<&Pubkey>,
    payer: &Pubkey,
) -> solana_sdk::instruction::Instruction {
    let (tree_config, _) = derive_tree_config(tree_pubkey);

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
        .payer(*payer)
        .tree_creator_or_delegate(Some(*tee_signing_pubkey))
        .leaf_owner(*creator_wallet)
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

/// 複数の MintV2 instruction を VersionedTransaction (v0) + ALT でビンパッキングする。
/// 仕様書 §5.1 Step 9-10
///
/// ALT によりアカウント参照が 32 bytes → 1 byte に圧縮され、
/// 1 TX あたり 6-7 cNFT をパッキング可能。
pub fn pack_mint_txs(
    instructions: Vec<solana_sdk::instruction::Instruction>,
    payer: &Pubkey,
    blockhash: &solana_sdk::hash::Hash,
    alt_account: &AddressLookupTableAccount,
) -> Vec<VersionedTransaction> {
    const MAX_TX_SIZE: usize = 1232;
    let alt_accounts = std::slice::from_ref(alt_account);

    let mut txs = Vec::new();
    let mut current_ixs: Vec<solana_sdk::instruction::Instruction> = Vec::new();

    for ix in instructions {
        current_ixs.push(ix);

        if let Ok(tx) = build_v0_tx(&current_ixs, payer, blockhash, alt_accounts) {
            let serialized = bincode::serialize(&tx).unwrap_or_default();
            if serialized.len() <= MAX_TX_SIZE {
                continue;
            }
        }

        // 収まらない → 最後の1つを除いてTXを確定
        let overflow = current_ixs.pop().unwrap();

        if !current_ixs.is_empty() {
            if let Ok(tx) = build_v0_tx(&current_ixs, payer, blockhash, alt_accounts) {
                txs.push(tx);
            }
        }

        current_ixs = vec![overflow];
    }

    // 残りを最後のTXに
    if !current_ixs.is_empty() {
        if let Ok(tx) = build_v0_tx(&current_ixs, payer, blockhash, alt_accounts) {
            txs.push(tx);
        }
    }

    txs
}

// ---------------------------------------------------------------------------
// 部分署名・シリアライズ
// ---------------------------------------------------------------------------

/// VersionedTransaction に部分署名を適用する。
/// 指定した公開鍵に対応する署名スロットにEd25519署名をセットする。
pub fn apply_partial_signature(
    tx: &mut VersionedTransaction,
    pubkey: &Pubkey,
    signature_bytes: &[u8],
) -> Result<(), String> {
    let sig_arr: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| "署名は64バイトである必要があります".to_string())?;
    let signature = Signature::from(sig_arr);

    let num_signers = tx.message.header().num_required_signatures as usize;
    let static_keys = tx.message.static_account_keys();
    for i in 0..num_signers {
        if i < static_keys.len() && static_keys[i] == *pubkey {
            tx.signatures[i] = signature;
            return Ok(());
        }
    }

    Err(format!(
        "公開鍵 {} がトランザクションの署名者に見つかりません",
        pubkey
    ))
}

/// VersionedTransaction をバイナリにシリアライズする。
pub fn serialize_transaction(tx: &VersionedTransaction) -> Result<Vec<u8>, String> {
    bincode::serialize(tx).map_err(|e| format!("トランザクションのシリアライズに失敗: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ix_size_breakdown() {
        let tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let col = Pubkey::new_unique();

        let prod_uri = "https://gateway.irys.xyz/Abc123defGHI456jklMNO789pqrSTU012vwxYZ3456789";
        let ix = build_mint_v2_ix(
            &tree, &tee_signer, &creator,
            "0xabcdef1234567890", prod_uri, Some(&col), &creator,
        );

        eprintln!("=== MintV2 Instruction 内訳 ===");
        eprintln!("  program_id: 32 bytes");
        eprintln!("  accounts: {} 個 × (pubkey(32) + is_signer(1) + is_writable(1)) = {} bytes (ALT前)",
            ix.accounts.len(), ix.accounts.len() * 34);
        eprintln!("  accounts (ALT後): {} 個 → 署名者以外はインデックス1byte",
            ix.accounts.len());
        eprintln!("  data: {} bytes", ix.data.len());
        eprintln!("  URI: {} bytes (\"{}\")", prod_uri.len(), prod_uri);
        eprintln!("");

        for (i, acc) in ix.accounts.iter().enumerate() {
            let role = if acc.is_signer && acc.is_writable { "signer+mut" }
                else if acc.is_signer { "signer" }
                else if acc.is_writable { "mut" }
                else { "readonly" };
            eprintln!("  account[{i}]: {role:12} {}", acc.pubkey);
        }

        eprintln!("");
        eprintln!("=== V0 TX サイズ見積もり (1 ix) ===");
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[tree], &[col]);

        // 1 ix
        let txs1 = pack_mint_txs(vec![ix.clone()], &creator, &blockhash, &alt);
        let s1 = bincode::serialize(&txs1[0]).unwrap().len();

        // ALTなしで比較
        let tx_no_alt = build_v0_tx(&[ix.clone()], &creator, &blockhash, &[]).unwrap();
        let s_no_alt = bincode::serialize(&tx_no_alt).unwrap().len();

        eprintln!("  ALT なし: {} bytes", s_no_alt);
        eprintln!("  ALT あり: {} bytes (削減: {} bytes)", s1, s_no_alt - s1);

        // 2 ix
        let ix2 = build_mint_v2_ix(
            &tree, &tee_signer, &creator,
            "0x1234567890abcdef", prod_uri, Some(&col), &creator,
        );
        let txs2 = pack_mint_txs(vec![ix.clone(), ix2.clone()], &creator, &blockhash, &alt);
        let s2 = bincode::serialize(&txs2[0]).unwrap().len();
        eprintln!("  2 ix ALT: {} bytes (追加 ix あたり: {} bytes)", s2, s2 - s1);

        // 3 ix
        let ix3 = build_mint_v2_ix(
            &tree, &tee_signer, &creator,
            "0xdeadbeef12345678", prod_uri, Some(&col), &creator,
        );
        let txs3 = pack_mint_txs(vec![ix, ix2, ix3], &creator, &blockhash, &alt);
        let s3 = bincode::serialize(&txs3[0]).unwrap().len();
        eprintln!("  3 ix ALT: {} bytes (追加 ix あたり: {} bytes)", s3, s3 - s2);
        eprintln!("  残り headroom: {} bytes", 1232 - s3);
    }

    /// テスト用ALTアカウントを作成する。
    fn test_alt(
        trees: &[Pubkey],
        collections: &[Pubkey],
    ) -> AddressLookupTableAccount {
        let mut addresses = vec![
            mpl_bubblegum::ID,
            spl_account_compression_v2_id(),
            solana_sdk::system_program::id(),
            Pubkey::from_str("noopb9bkMVfRPU8AsBRBV8TihGZw1GW7RL9kY5mn2Mb").unwrap(),
            Pubkey::from_str("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap(),
            derive_mpl_core_cpi_signer().0,
        ];
        for tree in trees {
            addresses.push(*tree);
            addresses.push(derive_tree_config(tree).0);
        }
        for col in collections {
            addresses.push(*col);
        }
        AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses,
        }
    }

    #[test]
    fn test_merkle_tree_account_size() {
        assert_eq!(merkle_tree_account_size(14, 64), 31800);
        assert_eq!(merkle_tree_account_size(20, 64), 44280);
        assert_eq!(merkle_tree_account_size(20, 1024), 697080);
    }

    #[test]
    fn test_derive_tree_config() {
        let tree = Pubkey::new_unique();
        let (config, bump) = derive_tree_config(&tree);
        assert_ne!(config, tree);
        let (config2, bump2) = derive_tree_config(&tree);
        assert_eq!(config, config2);
        assert_eq!(bump, bump2);
    }

    #[test]
    fn test_derive_mpl_core_cpi_signer() {
        let (signer, _) = derive_mpl_core_cpi_signer();
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
        assert_eq!(tx.message.header().num_required_signatures, 3);

        // シリアライズ可能
        let bytes = serialize_transaction(&tx).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_pack_single_mint_v0() {
        let tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[tree], &[]);

        let ix = build_mint_v2_ix(
            &tree, &tee_signer, &creator,
            "0x1234abcdef567890", "ar://test_uri", None, &creator,
        );
        let txs = pack_mint_txs(vec![ix], &creator, &blockhash, &alt);

        assert_eq!(txs.len(), 1);
        let size = bincode::serialize(&txs[0]).unwrap().len();
        eprintln!("v0 single mint: {} bytes", size);
        assert!(size <= 1232);
    }

    #[test]
    fn test_pack_two_mints_v0_with_alt() {
        let tree1 = Pubkey::new_unique();
        let tree2 = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let col1 = Pubkey::new_unique();
        let col2 = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[tree1, tree2], &[col1, col2]);

        let ix1 = build_mint_v2_ix(
            &tree1, &tee_signer, &creator,
            "0x1234abcdef567890", "ar://u1", Some(&col1), &creator,
        );
        let ix2 = build_mint_v2_ix(
            &tree2, &tee_signer, &creator,
            "0x1234abcdef567890", "ar://u2", Some(&col2), &creator,
        );
        let txs = pack_mint_txs(vec![ix1, ix2], &creator, &blockhash, &alt);

        assert_eq!(txs.len(), 1);
        let size = bincode::serialize(&txs[0]).unwrap().len();
        eprintln!("v0 two mints with ALT: {} bytes", size);
        assert!(size <= 1232);
    }

    #[test]
    fn test_pack_with_prod_uris() {
        let tree1 = Pubkey::new_unique();
        let tree2 = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let col1 = Pubkey::new_unique();
        let col2 = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[tree1, tree2], &[col1, col2]);

        let uri1 = "https://gateway.irys.xyz/Abc123defGHI456jklMNO789pqrSTU012vwxYZ3456789";
        let uri2 = "https://gateway.irys.xyz/Xyz987wvuTSR654ponMLK321jihGFE098dcbAZY7654321";

        let ix1 = build_mint_v2_ix(
            &tree1, &tee_signer, &creator,
            "0xabcdef1234567890", uri1, Some(&col1), &creator,
        );
        let ix2 = build_mint_v2_ix(
            &tree2, &tee_signer, &creator,
            "0x1234567890abcdef", uri2, Some(&col2), &creator,
        );
        let txs = pack_mint_txs(vec![ix1, ix2], &creator, &blockhash, &alt);

        assert_eq!(txs.len(), 1, "prod URIでも2 ixが1 TXに収まるべき");
        let size = bincode::serialize(&txs[0]).unwrap().len();
        eprintln!("v0 prod URIs with ALT: {} bytes (headroom: {} bytes)", size, 1232 - size);
        assert!(size <= 1232);
    }

    #[test]
    fn test_pack_six_plus_mints_with_alt() {
        // 6+ cNFT が ALT により1 TXに収まることを確認
        let core_tree = Pubkey::new_unique();
        let ext_tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let core_col = Pubkey::new_unique();
        let ext_col = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[core_tree, ext_tree], &[core_col, ext_col]);

        // 短いURI（テスト用）で6 cNFTを生成
        let mut ixs = Vec::new();
        for i in 0..6 {
            let tree = if i % 2 == 0 { &core_tree } else { &ext_tree };
            let col = if i % 2 == 0 { Some(&core_col) } else { Some(&ext_col) };
            let uri = format!("ar://test_uri_{i}");
            let hash = format!("0x{i:016x}");
            ixs.push(build_mint_v2_ix(
                tree, &tee_signer, &creator, &hash, &uri, col, &creator,
            ));
        }

        let txs = pack_mint_txs(ixs, &creator, &blockhash, &alt);
        let total_ixs: usize = txs.iter().map(|t| {
            let size = bincode::serialize(t).unwrap().len();
            eprintln!("tx: {} bytes", size);
            match &t.message {
                VersionedMessage::V0(m) => m.instructions.len(),
                _ => 0,
            }
        }).sum();
        assert_eq!(total_ixs, 6);
        eprintln!("6 short-URI mints packed into {} TX(s)", txs.len());
        // ALTにより6 ixが2 TX以内に収まる（旧: 6 TX必要だった）
        assert!(txs.len() <= 2);
    }

    #[test]
    fn test_pack_many_mints_prod_uris() {
        // 本番URIで6+ cNFTのパッキング確認
        let core_tree = Pubkey::new_unique();
        let ext_tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let core_col = Pubkey::new_unique();
        let ext_col = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[core_tree, ext_tree], &[core_col, ext_col]);

        // Irys本番URI (~69 bytes) で最大数をテスト
        let mut ixs = Vec::new();
        for i in 0..8 {
            let tree = if i % 2 == 0 { &core_tree } else { &ext_tree };
            let col = if i % 2 == 0 { Some(&core_col) } else { Some(&ext_col) };
            let uri = format!("https://gateway.irys.xyz/{:0>43}", i);
            let hash = format!("0x{i:016x}");
            ixs.push(build_mint_v2_ix(
                tree, &tee_signer, &creator, &hash, &uri, col, &creator,
            ));
        }

        let txs = pack_mint_txs(ixs, &creator, &blockhash, &alt);
        let mut total_ixs = 0;
        for (j, tx) in txs.iter().enumerate() {
            let size = bincode::serialize(tx).unwrap().len();
            let n = match &tx.message {
                VersionedMessage::V0(m) => m.instructions.len(),
                _ => 0,
            };
            total_ixs += n;
            eprintln!("tx[{j}]: {n} instructions, {size} bytes");
        }
        assert_eq!(total_ixs, 8);
        eprintln!("8 prod-URI mints packed into {} TX(s)", txs.len());
        // 本番URIでも少なくとも4+ per TX（ALTにより大幅削減）
        assert!(txs.len() <= 2, "ALTにより8 ixが2 TX以内に収まるべき");
    }

    #[test]
    fn test_apply_partial_signature() {
        let tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[tree], &[]);

        let ix = build_mint_v2_ix(
            &tree, &tee_signer, &creator,
            "0x1234abcdef567890", "ar://test_uri", None, &creator,
        );
        let mut txs = pack_mint_txs(vec![ix], &creator, &blockhash, &alt);
        assert_eq!(txs.len(), 1);

        let dummy_sig = [1u8; 64];
        let result = apply_partial_signature(&mut txs[0], &tee_signer, &dummy_sig);
        assert!(result.is_ok());

        let unknown = Pubkey::new_unique();
        let result = apply_partial_signature(&mut txs[0], &unknown, &dummy_sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_partial_signature_create_tree() {
        let payer = Pubkey::new_unique();
        let tree = Pubkey::new_unique();
        let tree_creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();

        let mut tx = build_create_tree_tx(&payer, &tree, &tree_creator, 20, 64, &blockhash);

        let dummy_sig = [1u8; 64];
        let result = apply_partial_signature(&mut tx, &tree_creator, &dummy_sig);
        assert!(result.is_ok());

        let unknown = Pubkey::new_unique();
        let result = apply_partial_signature(&mut tx, &unknown, &dummy_sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_transaction() {
        let tree = Pubkey::new_unique();
        let tee_signer = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let blockhash = solana_sdk::hash::Hash::new_unique();
        let alt = test_alt(&[tree], &[]);

        let ix = build_mint_v2_ix(
            &tree, &tee_signer, &creator,
            "0x1234abcdef567890", "ar://test_uri", None, &creator,
        );
        let txs = pack_mint_txs(vec![ix], &creator, &blockhash, &alt);
        let bytes = serialize_transaction(&txs[0]);
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }
}
