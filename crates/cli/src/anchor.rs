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

/// MPL Core CreateCollectionV2 命令を手動構築する。
///
/// MPL Core プログラムのCreateCollectionV2命令:
///   discriminator: [43, 220, 59, 207, 220, 2, 68, 240]  (固定)
///   data: Borsh { name: String, uri: String, plugins: Option<Vec<_>>, external_plugins: Option<Vec<_>> }
///   accounts: [collection(signer+mut), payer(signer+mut), system_program]
pub fn build_create_collection_ix(
    collection: &Pubkey,
    payer: &Pubkey,
    name: &str,
    uri: &str,
) -> Instruction {
    // MPL Core program ID
    let mpl_core_program =
        Pubkey::try_from("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap();

    // CreateCollectionV2 discriminator (fixed)
    let disc: [u8; 8] = [43, 220, 59, 207, 220, 2, 68, 240];

    let mut data = Vec::new();
    data.extend_from_slice(&disc);
    data.extend_from_slice(&borsh_string(name));
    data.extend_from_slice(&borsh_string(uri));
    // plugins: Option<Vec<_>> = None
    data.push(0);
    // external_plugins: Option<Vec<_>> = None
    data.push(0);

    Instruction {
        program_id: mpl_core_program,
        accounts: vec![
            AccountMeta::new(*collection, true),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(system_program::id(), false),
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
        let program_id: Pubkey = "CD3KZe1NWppgkYSPJTq9g2JVYFBnm6ysGD1af8vJQMJq"
            .parse()
            .unwrap();
        let (pda, bump) = find_global_config_pda(&program_id);
        // PDA は有効（カーブ外）
        assert_ne!(bump, 0);
        assert_ne!(pda, Pubkey::default());
    }

    #[test]
    fn test_find_tee_node_pda() {
        let program_id: Pubkey = "CD3KZe1NWppgkYSPJTq9g2JVYFBnm6ysGD1af8vJQMJq"
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
}
