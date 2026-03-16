// SPDX-License-Identifier: Apache-2.0

//! `title-cli remove-wasm` サブコマンド。
//!
//! WASMモジュールをオンチェーンから削除する。
//! WasmModuleAccount PDAをクローズし、GlobalConfigのtrusted_wasm_idsから除去する。
//! authority keypairが必須。
//!
//! 仕様書 §7.3

use std::path::Path;

#[allow(deprecated)]
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signer::Signer,
    system_program,
    transaction::Transaction,
};

use crate::anchor;
use crate::config;
use crate::error::CliError;
use crate::rpc::SolanaRpc;

/// remove-wasm サブコマンドを実行する。
#[allow(deprecated)]
pub async fn run(
    project_root: &Path,
    keys_dir: &Path,
    extension_id: &str,
) -> Result<(), CliError> {
    println!("[remove-wasm] WASMモジュール削除...");

    let network_path = project_root.join("network.json");
    let network = config::load_network_config(&network_path)?;
    let rpc_url = config::resolve_rpc_url(&network.cluster, None);
    let rpc = SolanaRpc::new(&rpc_url);

    let program_id: Pubkey = network
        .program_id
        .parse()
        .map_err(|e| CliError::Config(format!("program_idのパースに失敗: {e}")))?;

    let extension_id_bytes = anchor::extension_id_bytes(extension_id);

    // Authority keypair
    let authority_key_path = config::resolve_key_path(keys_dir, "authority.json");
    if !authority_key_path.exists() {
        return Err(CliError::Config(
            "authority.json が見つかりません。remove-wasm には authority keypair が必要です。"
                .into(),
        ));
    }
    let authority = config::load_keypair(&authority_key_path)?
        .ok_or_else(|| CliError::Config("Authority keypairのロードに失敗".into()))?;
    let authority_pubkey = authority.pubkey();

    let (global_config_pda, _) = anchor::find_global_config_pda(&program_id);
    let (wasm_module_pda, _) = anchor::find_wasm_module_pda(&extension_id_bytes, &program_id);

    println!("  Extension ID:    {extension_id}");
    println!("  WASM Module PDA: {wasm_module_pda}");

    // PDA存在確認
    let pda_data = rpc.get_account_data(&wasm_module_pda).await?;
    if pda_data.is_none() {
        println!("  PDA が存在しません。既に削除済みです。");
        return Ok(());
    }

    // remove_wasm_module 命令を構築
    let mut data = Vec::new();
    data.extend_from_slice(&anchor::anchor_discriminator("remove_wasm_module"));

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(global_config_pda, false),
            AccountMeta::new(wasm_module_pda, false),
            AccountMeta::new(authority_pubkey, true),
            AccountMeta::new(authority_pubkey, false), // rent_recipient
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    };

    let blockhash = rpc.get_latest_blockhash().await?;
    let message = Message::new_with_blockhash(&[ix], Some(&authority_pubkey), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[&authority], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

    let sig = rpc.send_and_confirm(&tx_bytes).await?;
    println!("  WASMモジュール削除完了: {sig}");

    Ok(())
}
