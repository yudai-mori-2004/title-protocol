// SPDX-License-Identifier: Apache-2.0

//! `title-cli remove-node` サブコマンド。
//!
//! TEEノードをオンチェーンのGlobalConfigから削除する。
//! TeeNodeAccount PDAをクローズし、trusted_node_keysリストからも除去する。
//! authority keypairが必須。
//!
//! 仕様書 §8.2 TEEノードの削除

use std::path::Path;

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

/// remove-node サブコマンドを実行する。
///
/// signing_pubkey で指定されたTEEノードを GlobalConfig から削除し、
/// TeeNodeAccount PDA をクローズする。rent は authority に返還される。
#[allow(deprecated)]
pub async fn run(
    project_root: &Path,
    keys_dir: &Path,
    signing_pubkey_str: &str,
) -> Result<(), CliError> {
    println!("[remove-node] TEEノード削除...");

    // network.json 読み込み
    let network_path = project_root.join("network.json");
    let network = config::load_network_config(&network_path)?;
    let rpc_url = config::resolve_rpc_url(&network.cluster, None);
    let rpc = SolanaRpc::new(&rpc_url);

    let program_id: Pubkey = network
        .program_id
        .parse()
        .map_err(|e| CliError::Config(format!("program_idのパースに失敗: {e}")))?;

    // signing_pubkey のパース
    let signing_pubkey: Pubkey = signing_pubkey_str
        .parse()
        .map_err(|e| CliError::Config(format!("signing_pubkeyのパースに失敗: {e}")))?;

    println!("  Signing Pubkey: {signing_pubkey}");

    // TeeNodeAccount PDA 導出
    let signing_pubkey_bytes = signing_pubkey.to_bytes();
    let (tee_node_pda, _) = anchor::find_tee_node_pda(&signing_pubkey_bytes, &program_id);
    println!("  TEE Node PDA:   {tee_node_pda}");

    // PDAの存在確認
    let pda_data = rpc.get_account_data(&tee_node_pda).await?;
    if pda_data.is_none() {
        println!("  TEE Node PDA が存在しません。既に削除済みか、signing_pubkey が不正です。");
        return Ok(());
    }

    // Authority keypair の読み込み（必須）
    let authority_key_path = config::resolve_key_path(keys_dir, "authority.json");
    if !authority_key_path.exists() {
        return Err(CliError::Config(
            "authority.json が見つかりません。remove-node には authority keypair が必要です。\n  \
             keys/authority.json を配置してください。"
                .into(),
        ));
    }
    let authority = config::load_keypair(&authority_key_path)?
        .ok_or_else(|| CliError::Config("Authority keypairのロードに失敗".into()))?;
    let authority_pubkey = authority.pubkey();
    println!("  Authority:      {authority_pubkey}");

    // GlobalConfig PDA 導出
    let (global_config_pda, _) = anchor::find_global_config_pda(&program_id);

    // コレクションMintアドレス
    let core_collection: Pubkey = network
        .core_collection_mint
        .parse()
        .map_err(|e| CliError::Config(format!("core_collection_mintのパースに失敗: {e}")))?;
    let ext_collection: Pubkey = network
        .ext_collection_mint
        .parse()
        .map_err(|e| CliError::Config(format!("ext_collection_mintのパースに失敗: {e}")))?;

    // remove_tee_node 命令を構築
    let ix = anchor::build_remove_tee_node_ix(
        &program_id,
        &global_config_pda,
        &tee_node_pda,
        &authority_pubkey,
        &authority_pubkey, // rent返還先 = authority
        &core_collection,
        &ext_collection,
    );

    // 署名+ブロードキャスト
    let blockhash = rpc.get_latest_blockhash().await?;
    let message = Message::new_with_blockhash(&[ix], Some(&authority_pubkey), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[&authority], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

    match rpc.send_and_confirm(&tx_bytes).await {
        Ok(sig) => {
            println!("  TEEノード削除完了: {sig}");
            println!("  ノードの signing_pubkey は GlobalConfig から除去されました。");
            println!("  TeeNodeAccount PDA はクローズされ、rent が返還されました。");
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("AccountNotFound") || msg.contains("not found") {
                println!("  TEE Node PDA が見つかりません（既に削除済み）。");
            } else {
                return Err(e);
            }
        }
    }

    Ok(())
}
