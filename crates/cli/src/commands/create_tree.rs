// SPDX-License-Identifier: Apache-2.0

//! `title-cli create-tree` サブコマンド。
//!
//! Core + Extension Merkle Treeを作成する。
//! TEEが署名済みのTree作成TXを返却し、CLIがブロードキャストする。

use std::path::Path;

use base64::Engine;

use title_types::{CreateTreeRequest, CreateTreeResponse};

use crate::config;
use crate::error::CliError;
use crate::helpers;
use crate::rpc::{b64, SolanaRpc};

/// create-tree サブコマンドを実行する。
pub async fn run(
    project_root: &Path,
    keys_dir: &Path,
    tee_url: &str,
    max_depth: u32,
    max_buffer_size: u32,
) -> Result<(), CliError> {
    println!("[create-tree] Merkle Tree 作成...");

    let network_path = project_root.join("network.json");
    let network = config::load_network_config(&network_path)?;
    let rpc_url = config::resolve_rpc_url(&network.cluster, None);
    let rpc = SolanaRpc::new(&rpc_url);

    // Blockhash取得
    let blockhash = rpc.get_latest_blockhash().await?;

    // /create-tree リクエスト
    let tree_request = CreateTreeRequest {
        max_depth,
        max_buffer_size,
        recent_blockhash: blockhash.to_string(),
    };

    let result: CreateTreeResponse =
        match helpers::call_tee_endpoint(tee_url, "/create-tree", &tree_request).await? {
            helpers::TeeCallResult::Success(r) => r,
            helpers::TeeCallResult::HttpError { status: 409, .. } => {
                println!("  Merkle Tree: 既に作成済み（スキップ）");
                return Ok(());
            }
            helpers::TeeCallResult::HttpError { status, body } => {
                println!("  Merkle Tree 作成に失敗: HTTP {status}: {}", &body[..body.len().min(100)]);
                return Ok(());
            }
            helpers::TeeCallResult::ConnectionFailed(msg) => {
                println!("  TEEに接続できません: {}", &msg[..msg.len().min(60)]);
                println!("  TEE起動後に再実行してください。");
                return Ok(());
            }
        };

    println!("  Core Tree:      {}", result.core_tree_address);
    println!("  Extension Tree: {}", result.ext_tree_address);
    println!("  Signing Pubkey: {}", result.signing_pubkey);

    // TEE walletにSOL送金（Tree作成のrent用）
    let tee_pk: solana_sdk::pubkey::Pubkey = result
        .signing_pubkey
        .parse()
        .map_err(|e| CliError::Config(format!("signing_pubkeyのパースに失敗: {e}")))?;

    helpers::fund_tee_wallet(&rpc, keys_dir, &tee_pk, 500_000_000).await?;

    // Core Tree ブロードキャスト
    let core_tx_bytes = b64()
        .decode(&result.core_signed_tx)
        .map_err(|e| CliError::Config(format!("core_signed_txのデコードに失敗: {e}")))?;
    match rpc.send_and_confirm(&core_tx_bytes).await {
        Ok(sig) => println!("  Core Merkle Tree 作成完了: {sig}"),
        Err(e) => println!("  Core Merkle Tree 失敗: {e}"),
    }

    // Extension Tree ブロードキャスト
    let ext_tx_bytes = b64()
        .decode(&result.ext_signed_tx)
        .map_err(|e| CliError::Config(format!("ext_signed_txのデコードに失敗: {e}")))?;
    match rpc.send_and_confirm(&ext_tx_bytes).await {
        Ok(sig) => println!("  Extension Merkle Tree 作成完了: {sig}"),
        Err(e) => println!("  Extension Merkle Tree 失敗: {e}"),
    }

    // tee-info.json を更新
    let tee_info_path = project_root
        .join("tests")
        .join("e2e")
        .join("fixtures")
        .join("tee-info.json");
    let mut info = config::load_tee_info(&tee_info_path)?;
    info.core_tree_address = Some(result.core_tree_address);
    info.ext_tree_address = Some(result.ext_tree_address);
    info.signing_pubkey = Some(result.signing_pubkey);
    info.encryption_pubkey = Some(result.encryption_pubkey);
    config::save_tee_info(&tee_info_path, &info)?;
    println!("  TEE情報を更新: {}", tee_info_path.display());

    Ok(())
}
