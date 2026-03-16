// SPDX-License-Identifier: Apache-2.0

//! `title-cli register-wasm` サブコマンド。
//!
//! WASMモジュールをオンチェーンに登録する。
//! WasmModuleAccount PDAを作成し、初期バージョン（version=1）を登録する。
//! authority keypairが必須。
//!
//! 仕様書 §7.3

use std::path::Path;

use sha2::{Digest, Sha256};
#[allow(deprecated)]
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    signer::Signer,
    transaction::Transaction,
};

use crate::anchor;
use crate::config;
use crate::error::CliError;
use crate::rpc::SolanaRpc;

/// register-wasm サブコマンドを実行する。
#[allow(deprecated)]
pub async fn run(
    project_root: &Path,
    keys_dir: &Path,
    extension_id: &str,
    wasm_path: &Path,
    wasm_source: &str,
) -> Result<(), CliError> {
    println!("[register-wasm] WASMモジュール登録...");

    // network.json 読み込み
    let network_path = project_root.join("network.json");
    let network = config::load_network_config(&network_path)?;
    let rpc_url = config::resolve_rpc_url(&network.cluster, None);
    let rpc = SolanaRpc::new(&rpc_url);

    let program_id: Pubkey = network
        .program_id
        .parse()
        .map_err(|e| CliError::Config(format!("program_idのパースに失敗: {e}")))?;

    // WASMバイナリ読み込み + ハッシュ計算
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| CliError::Config(format!("WASMファイルの読み取りに失敗: {e}")))?;
    let wasm_hash: [u8; 32] = Sha256::digest(&wasm_bytes).into();
    let hash_hex = hex::encode(wasm_hash);

    let extension_id_bytes = anchor::extension_id_bytes(extension_id);

    println!("  Extension ID:   {extension_id}");
    println!("  WASM path:      {}", wasm_path.display());
    println!("  WASM size:      {} bytes", wasm_bytes.len());
    println!("  WASM hash:      {hash_hex}");
    println!("  WASM source:    {wasm_source}");

    // Authority keypair の読み込み
    let authority_key_path = config::resolve_key_path(keys_dir, "authority.json");
    if !authority_key_path.exists() {
        return Err(CliError::Config(
            "authority.json が見つかりません。register-wasm には authority keypair が必要です。"
                .into(),
        ));
    }
    let authority = config::load_keypair(&authority_key_path)?
        .ok_or_else(|| CliError::Config("Authority keypairのロードに失敗".into()))?;
    let authority_pubkey = authority.pubkey();

    // PDA 導出
    let (global_config_pda, _) = anchor::find_global_config_pda(&program_id);
    let (wasm_module_pda, _) = anchor::find_wasm_module_pda(&extension_id_bytes, &program_id);
    println!("  WASM Module PDA: {wasm_module_pda}");

    // PDA存在確認
    let pda_data = rpc.get_account_data(&wasm_module_pda).await?;
    if pda_data.is_some() {
        println!("  WASMモジュール PDA が既に存在します。add-wasm-version を使用してください。");
        return Ok(());
    }

    // register_wasm_module 命令を構築
    let ix = anchor::build_register_wasm_module_ix(
        &program_id,
        &global_config_pda,
        &wasm_module_pda,
        &authority_pubkey,
        &extension_id_bytes,
        &wasm_hash,
        wasm_source,
    );

    let blockhash = rpc.get_latest_blockhash().await?;
    let message = Message::new_with_blockhash(&[ix], Some(&authority_pubkey), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[&authority], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

    let sig = rpc.send_and_confirm(&tx_bytes).await?;
    println!("  WASMモジュール登録完了: {sig}");
    println!("  Extension ID '{extension_id}' が GlobalConfig に追加されました。");
    println!("  WasmModuleAccount PDA にバージョン 1 が登録されました。");

    Ok(())
}
