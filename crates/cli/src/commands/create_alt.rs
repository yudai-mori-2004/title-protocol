// SPDX-License-Identifier: Apache-2.0

//! `title-cli create-alt` サブコマンド。
//!
//! Address Lookup Table を作成し、MintV2 で使われるアカウントを登録する。
//! 作成後、TEE の /set-alt に通知して ALT 情報を設定する。

use std::path::Path;

#[allow(deprecated)]
use solana_sdk::{
    address_lookup_table,
    message::Message,
    pubkey::Pubkey,
    signer::Signer,
    transaction::Transaction,
};
use std::str::FromStr;

use crate::config;
use crate::error::CliError;
use crate::rpc::SolanaRpc;

/// Bubblegum V2 プログラムID。
fn bubblegum_id() -> Pubkey {
    Pubkey::from_str("BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752kRSfkm").unwrap()
}

/// tree_config PDA を導出する。
fn derive_tree_config(tree: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[tree.as_ref()], &bubblegum_id()).0
}

/// MPL Core CPI Signer PDA を導出する。
fn derive_mpl_core_cpi_signer() -> Pubkey {
    Pubkey::find_program_address(&[b"mpl_core_cpi_signer"], &bubblegum_id()).0
}

/// create-alt サブコマンドを実行する。
pub async fn run(
    project_root: &Path,
    keys_dir: &Path,
    tee_url: &str,
) -> Result<(), CliError> {
    println!("[create-alt] Address Lookup Table 作成...");

    let network_path = project_root.join("network.json");
    let network = config::load_network_config(&network_path)?;
    let rpc_url = config::resolve_rpc_url(&network.cluster, None);
    let rpc = SolanaRpc::new(&rpc_url);

    // operator keypair
    let operator_path = keys_dir.join("operator.json");
    let operator = config::load_keypair(&operator_path)?
        .ok_or_else(|| CliError::Config("keys/operator.json のロードに失敗".into()))?;

    // tee-info.json から Tree アドレスを取得
    let tee_info_path = project_root
        .join("tests")
        .join("e2e")
        .join("fixtures")
        .join("tee-info.json");
    let info = config::load_tee_info(&tee_info_path)?;

    let core_tree_str = info.core_tree_address.as_ref()
        .ok_or_else(|| CliError::Config("core_tree_address が tee-info.json にありません。先に create-tree を実行してください".into()))?;
    let ext_tree_str = info.ext_tree_address.as_ref()
        .ok_or_else(|| CliError::Config("ext_tree_address が tee-info.json にありません".into()))?;

    let core_tree = Pubkey::from_str(core_tree_str)
        .map_err(|e| CliError::Config(format!("core_tree_address のパースに失敗: {e}")))?;
    let ext_tree = Pubkey::from_str(ext_tree_str)
        .map_err(|e| CliError::Config(format!("ext_tree_address のパースに失敗: {e}")))?;

    println!("  Core Tree:  {core_tree}");
    println!("  Ext Tree:   {ext_tree}");

    // コレクションMint
    let core_collection = network.core_collection_mint.parse::<Pubkey>()
        .map_err(|e| CliError::Config(format!("core_collection_mint パースに失敗: {e}")))?;
    let ext_collection = network.ext_collection_mint.parse::<Pubkey>()
        .map_err(|e| CliError::Config(format!("ext_collection_mint パースに失敗: {e}")))?;

    // ALT に登録するアカウント一覧
    let compression_v2 = Pubkey::from_str("mcmt6YrQEMKw8Mw43FmpRLmf7BqRnFMKmAcbxE3xkAW").unwrap();
    let noop = Pubkey::from_str("noopb9bkMVfRPU8AsBRBV8TihGZw1GW7RL9kY5mn2Mb").unwrap();
    let mpl_core = Pubkey::from_str("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap();

    let alt_addresses = vec![
        bubblegum_id(),
        compression_v2,
        solana_sdk::system_program::id(),
        noop,
        mpl_core,
        derive_mpl_core_cpi_signer(),
        core_tree,
        derive_tree_config(&core_tree),
        ext_tree,
        derive_tree_config(&ext_tree),
        core_collection,
        ext_collection,
    ];

    println!("  ALT アカウント数: {}", alt_addresses.len());

    // Step 1: ALT 作成
    let recent_slot = get_recent_slot(&rpc).await?;
    let (create_ix, alt_pubkey) =
        address_lookup_table::instruction::create_lookup_table(
            operator.pubkey(),
            operator.pubkey(),
            recent_slot,
        );

    println!("  ALT アドレス: {alt_pubkey}");

    let blockhash = rpc.get_latest_blockhash().await?;
    let message = Message::new_with_blockhash(&[create_ix], Some(&operator.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[&operator], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;
    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

    match rpc.send_and_confirm(&tx_bytes).await {
        Ok(sig) => println!("  ALT 作成完了: {sig}"),
        Err(e) => {
            // ALT が既に存在する場合は create が失敗するが、extend は可能
            println!("  ALT 作成スキップ（既存の可能性）: {e}");
        }
    }

    // Step 2: ALT にアカウントを追加
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let extend_ix = address_lookup_table::instruction::extend_lookup_table(
        alt_pubkey,
        operator.pubkey(),
        Some(operator.pubkey()),
        alt_addresses.clone(),
    );

    let blockhash = rpc.get_latest_blockhash().await?;
    let message = Message::new_with_blockhash(&[extend_ix], Some(&operator.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[&operator], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;
    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

    match rpc.send_and_confirm(&tx_bytes).await {
        Ok(sig) => println!("  ALT extend 完了: {sig}"),
        Err(e) => return Err(CliError::Transaction(format!("ALT extend 失敗: {e}"))),
    }

    // Step 3: TEE に通知
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let addr_strings: Vec<String> = alt_addresses.iter().map(|p| p.to_string()).collect();
    let set_alt_body = serde_json::json!({
        "alt_address": alt_pubkey.to_string(),
        "addresses": addr_strings,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{tee_url}/set-alt"))
        .json(&set_alt_body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            println!("  TEE ALT 設定完了");
        }
        Ok(r) => {
            let body = r.text().await.unwrap_or_default();
            println!("  TEE ALT 設定失敗: {body}");
        }
        Err(e) => {
            println!("  TEE ALT 設定失敗（接続エラー）: {e}");
        }
    }

    println!("  ALT: {alt_pubkey}");
    Ok(())
}

/// 最新のスロットを取得する。
async fn get_recent_slot(rpc: &SolanaRpc) -> Result<u64, CliError> {
    // getSlot RPC を直接呼ぶ
    let client = reqwest::Client::new();
    let resp = client
        .post(rpc.url().to_string())
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSlot",
            "params": [{"commitment": "confirmed"}]
        }))
        .send()
        .await
        .map_err(|e| CliError::Rpc(format!("getSlot に失敗: {e}")))?;
    let body: serde_json::Value = resp.json().await
        .map_err(|e| CliError::Rpc(format!("getSlot レスポンスのパースに失敗: {e}")))?;
    body.get("result")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::Rpc("getSlot の結果が不正です".into()))
}
