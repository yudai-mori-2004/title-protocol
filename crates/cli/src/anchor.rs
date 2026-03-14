// SPDX-License-Identifier: Apache-2.0

//! Anchor 命令構築ヘルパー。
//!
//! `crates/tee/src/endpoints/register_node.rs` のパターンを再利用。

use sha2::{Digest, Sha256};
#[allow(deprecated)]
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use title_types::ResourceLimits;

/// Anchor instruction discriminator: sha256("global:<method>")[..8]
pub fn anchor_discriminator(method: &str) -> [u8; 8] {
    let hash = Sha256::digest(format!("global:{method}").as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

/// GlobalConfig PDA導出。seeds = [b"global-config"]
pub fn find_global_config_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"global-config"], program_id)
}

/// TeeNodeAccount PDA導出。seeds = [b"tee-node", &signing_pubkey]
#[allow(dead_code)]
pub fn find_tee_node_pda(signing_pubkey: &[u8; 32], program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"tee-node", signing_pubkey.as_ref()], program_id)
}

/// Borsh String encode: 4-byte LE length + UTF-8 bytes
pub fn borsh_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut buf = Vec::with_capacity(4 + bytes.len());
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    buf
}

/// Extension IDを32バイトに右0パディング。
pub fn extension_id_bytes(id: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let bytes = id.as_bytes();
    let len = bytes.len().min(32);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

/// `initialize` 命令を構築する。
/// 仕様書 §5.2 Step 1
#[allow(deprecated)]
pub fn build_initialize_ix(
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    authority: &Pubkey,
    core_collection_mint: &Pubkey,
    ext_collection_mint: &Pubkey,
) -> Instruction {
    let mut data = Vec::new();
    data.extend_from_slice(&anchor_discriminator("initialize"));
    data.extend_from_slice(&core_collection_mint.to_bytes());
    data.extend_from_slice(&ext_collection_mint.to_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*global_config_pda, false),
            AccountMeta::new(*authority, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// `update_collections` 命令を構築する。
/// 仕様書 §5.2 Step 1
pub fn build_update_collections_ix(
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    authority: &Pubkey,
    core_collection_mint: &Pubkey,
    ext_collection_mint: &Pubkey,
) -> Instruction {
    let mut data = Vec::new();
    data.extend_from_slice(&anchor_discriminator("update_collections"));
    data.extend_from_slice(&core_collection_mint.to_bytes());
    data.extend_from_slice(&ext_collection_mint.to_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*global_config_pda, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// `add_wasm_module` 命令を構築する（upsert）。
/// 仕様書 §7.3
pub fn build_add_wasm_module_ix(
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    authority: &Pubkey,
    extension_id: &str,
    wasm_hash: &[u8; 32],
    wasm_source: &str,
) -> Instruction {
    let mut data = Vec::new();
    data.extend_from_slice(&anchor_discriminator("add_wasm_module"));
    data.extend_from_slice(&extension_id_bytes(extension_id));
    data.extend_from_slice(wasm_hash);
    data.extend_from_slice(&borsh_string(wasm_source));

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*global_config_pda, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// `remove_tee_node` 命令を構築する。
/// 仕様書 §8.2 TEEノードの削除
///
/// TeeNodeAccount PDA をクローズし、trusted_node_keys から signing_pubkey を除去する。
/// 同時にMPL CoreコレクションのUpdateDelegateプラグインを更新/削除する。
/// rent lamports は rent_recipient に返還される。
#[allow(deprecated)]
pub fn build_remove_tee_node_ix(
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    tee_node_pda: &Pubkey,
    authority: &Pubkey,
    rent_recipient: &Pubkey,
    core_collection_mint: &Pubkey,
    ext_collection_mint: &Pubkey,
) -> Instruction {
    let mpl_core_program =
        Pubkey::try_from("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap();

    let data = anchor_discriminator("remove_tee_node").to_vec();

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*global_config_pda, false),
            AccountMeta::new(*tee_node_pda, false),
            AccountMeta::new(*authority, true),
            AccountMeta::new(*rent_recipient, false),
            AccountMeta::new(*core_collection_mint, false),
            AccountMeta::new(*ext_collection_mint, false),
            AccountMeta::new_readonly(mpl_core_program, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// MPL Core CreateCollectionV2 命令を手動構築する。
///
/// MPL Core プログラムのCreateCollectionV2命令:
///   Borsh enum variant index: 21 (MplAssetInstruction::CreateCollectionV2)
///   data: Borsh { name: String, uri: String, plugins: Option<Vec<_>>, external_plugins: Option<Vec<_>> }
///   accounts: [collection(signer+mut), update_authority(optional), payer(signer+mut), system_program]
pub fn build_create_collection_ix(
    collection: &Pubkey,
    payer: &Pubkey,
    name: &str,
    uri: &str,
) -> Instruction {
    // MPL Core program ID
    let mpl_core_program =
        Pubkey::try_from("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap();

    // CreateCollectionV2: Borsh enum variant index 21
    // (MPL Core uses BorshDeserialize for instruction dispatch, not Anchor-style 8-byte discriminator)
    //
    // Bubblegum V2 はコレクションに BubblegumV2 プラグイン（permanent）を要求する。
    // このプラグインはコレクション作成時にのみ追加可能（後から AddCollectionPlugin では不可）。
    let bubblegum_program =
        Pubkey::try_from("BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY").unwrap();

    let mut data = Vec::new();
    data.push(21);
    data.extend_from_slice(&borsh_string(name));
    data.extend_from_slice(&borsh_string(uri));
    // plugins: Option<Vec<PluginAuthorityPair>> = Some(vec![BubblegumV2])
    data.push(1); // Option::Some
    data.extend_from_slice(&1u32.to_le_bytes()); // Vec len = 1
    // PluginAuthorityPair { plugin: Plugin::BubblegumV2(BubblegumV2 {}), authority: Some(Authority::Address { address }) }
    data.push(15); // Plugin::BubblegumV2 variant index (empty struct, no data)
    data.push(1);  // Option::Some for authority
    data.push(3);  // Authority::Address variant
    data.extend_from_slice(&bubblegum_program.to_bytes()); // 32 bytes
    // external_plugins: Option<Vec<_>> = None
    data.push(0);

    Instruction {
        program_id: mpl_core_program,
        accounts: vec![
            AccountMeta::new(*collection, true),
            AccountMeta::new_readonly(*payer, false), // update_authority (optional, defaults to payer)
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// Borsh Option<u64> encode: 0x00 for None, 0x01 + 8-byte LE for Some
fn borsh_option_u64(val: Option<u64>) -> Vec<u8> {
    match val {
        None => vec![0x00],
        Some(v) => {
            let mut buf = vec![0x01];
            buf.extend_from_slice(&v.to_le_bytes());
            buf
        }
    }
}

/// ResourceLimitsをBorshシリアライズする（7つのOption<u64>フィールド）。
fn borsh_resource_limits(limits: &ResourceLimits) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(borsh_option_u64(limits.max_single_content_bytes));
    buf.extend(borsh_option_u64(limits.max_concurrent_bytes));
    buf.extend(borsh_option_u64(limits.min_upload_speed_bytes));
    buf.extend(borsh_option_u64(limits.base_processing_time_sec));
    buf.extend(borsh_option_u64(limits.max_global_timeout_sec));
    buf.extend(borsh_option_u64(limits.chunk_read_timeout_sec));
    buf.extend(borsh_option_u64(limits.c2pa_max_graph_size));
    buf
}

/// `set_resource_limits` 命令を構築する。
/// 仕様書 §6.2: リソース制限のオンチェーン設定。
pub fn build_set_resource_limits_ix(
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    authority: &Pubkey,
    limits: &ResourceLimits,
) -> Instruction {
    let mut data = Vec::new();
    data.extend_from_slice(&anchor_discriminator("set_resource_limits"));
    data.extend(borsh_resource_limits(limits));

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*global_config_pda, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_discriminator() {
        let disc = anchor_discriminator("initialize");
        assert_eq!(disc.len(), 8);
        // discriminator は決定的
        assert_eq!(disc, anchor_discriminator("initialize"));
        // 異なるメソッドは異なるdisc
        assert_ne!(disc, anchor_discriminator("update_collections"));
    }

    #[test]
    fn test_find_global_config_pda() {
        let program_id: Pubkey = "5p5Tf93fEbCPZxA1NG48rH9ozDALsVmVVf52QW3VDNoN"
            .parse()
            .unwrap();
        let (pda, bump) = find_global_config_pda(&program_id);
        // PDA は有効（カーブ外）
        assert_ne!(bump, 0);
        assert_ne!(pda, Pubkey::default());
    }

    #[test]
    fn test_find_tee_node_pda() {
        let program_id: Pubkey = "5p5Tf93fEbCPZxA1NG48rH9ozDALsVmVVf52QW3VDNoN"
            .parse()
            .unwrap();
        let key = [42u8; 32];
        let (pda, _) = find_tee_node_pda(&key, &program_id);
        assert_ne!(pda, Pubkey::default());
    }

    #[test]
    fn test_borsh_string() {
        let encoded = borsh_string("hello");
        assert_eq!(&encoded[..4], &5u32.to_le_bytes());
        assert_eq!(&encoded[4..], b"hello");
    }

    #[test]
    fn test_extension_id_bytes() {
        let bytes = extension_id_bytes("phash-v1");
        assert_eq!(&bytes[..8], b"phash-v1");
        assert!(bytes[8..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_extension_id_bytes_exact_32() {
        let long = "a".repeat(32);
        let bytes = extension_id_bytes(&long);
        assert!(bytes.iter().all(|&b| b == b'a'));
    }

    #[test]
    fn test_borsh_option_u64_none() {
        let result = borsh_option_u64(None);
        assert_eq!(result, vec![0x00]);
    }

    #[test]
    fn test_borsh_option_u64_some() {
        let result = borsh_option_u64(Some(42));
        assert_eq!(result.len(), 9);
        assert_eq!(result[0], 0x01);
        assert_eq!(&result[1..], &42u64.to_le_bytes());
    }

    #[test]
    fn test_borsh_resource_limits() {
        let limits = ResourceLimits {
            max_single_content_bytes: Some(1024),
            max_concurrent_bytes: None,
            min_upload_speed_bytes: Some(512),
            base_processing_time_sec: None,
            max_global_timeout_sec: None,
            chunk_read_timeout_sec: None,
            c2pa_max_graph_size: Some(100),
        };
        let data = borsh_resource_limits(&limits);
        // Some(1024): 9B, None: 1B, Some(512): 9B, None×4: 4B, Some(100): 9B = 32B
        assert_eq!(data.len(), 9 + 1 + 9 + 1 + 1 + 1 + 9);
    }

    #[test]
    fn test_build_set_resource_limits_ix() {
        let program_id: Pubkey = "5p5Tf93fEbCPZxA1NG48rH9ozDALsVmVVf52QW3VDNoN"
            .parse()
            .unwrap();
        let (pda, _) = find_global_config_pda(&program_id);
        let authority = Pubkey::new_unique();
        let limits = ResourceLimits {
            max_single_content_bytes: Some(2 * 1024 * 1024 * 1024),
            max_concurrent_bytes: Some(8 * 1024 * 1024 * 1024),
            min_upload_speed_bytes: Some(1024 * 1024),
            base_processing_time_sec: Some(30),
            max_global_timeout_sec: Some(3600),
            chunk_read_timeout_sec: Some(30),
            c2pa_max_graph_size: Some(10000),
        };
        let ix = build_set_resource_limits_ix(&program_id, &pda, &authority, &limits);
        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 2);
        // discriminator(8) + 7 × Some(u64)(9) = 8 + 63 = 71
        assert_eq!(ix.data.len(), 8 + 7 * 9);
    }
}
