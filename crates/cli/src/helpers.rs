// SPDX-License-Identifier: Apache-2.0

//! CLI共通ヘルパー。
//!
//! 複数のサブコマンドで共有されるユーティリティ関数。

use std::path::Path;

#[allow(deprecated)]
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    signer::Signer,
    system_instruction,
    transaction::Transaction,
};

use crate::config;
use crate::error::CliError;
use crate::rpc::SolanaRpc;

/// ホームディレクトリを取得する。
pub fn dirs_home() -> Result<std::path::PathBuf, CliError> {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map_err(|_| CliError::Config("HOME環境変数が設定されていません".into()))
}

/// TEE walletにSOLを送金する。
///
/// authority keypair → Solana CLI wallet の順にfunderを探索する。
/// どちらも見つからない場合は手動送金を案内して正常終了する。
#[allow(deprecated)]
pub async fn fund_tee_wallet(
    rpc: &SolanaRpc,
    project_root: &Path,
    tee_pubkey: &Pubkey,
    required_lamports: u64,
) -> Result<(), CliError> {
    let authority_path = project_root
        .join("programs")
        .join("title-config")
        .join("keys")
        .join("authority.json");
    let wallet_path = dirs_home()?.join(".config").join("solana").join("id.json");

    let funder = if authority_path.exists() {
        config::load_keypair(&authority_path)?
    } else if wallet_path.exists() {
        config::load_keypair(&wallet_path)?
    } else {
        let sol = required_lamports as f64 / 1_000_000_000.0;
        println!("  WARNING: SOL送金元のキーペアが見つかりません。手動で送金してください:");
        println!("    solana transfer {tee_pubkey} {sol} --allow-unfunded-recipient");
        return Ok(());
    };

    let Some(funder) = funder else {
        println!("  WARNING: キーペアのロードに失敗。手動送金してください。");
        return Ok(());
    };

    let current_balance = rpc.get_balance(tee_pubkey).await?;
    if current_balance >= required_lamports {
        return Ok(());
    }
    let amount = required_lamports - current_balance;
    let amount_sol = amount as f64 / 1_000_000_000.0;
    println!("  TEE walletにSOL送金中... ({amount_sol:.2} SOL)");

    let ix = system_instruction::transfer(&funder.pubkey(), tee_pubkey, amount);
    let blockhash = rpc.get_latest_blockhash().await?;
    let message =
        Message::new_with_blockhash(&[ix], Some(&funder.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[&funder], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;
    let sig = rpc.send_and_confirm(&tx_bytes).await?;
    println!("  SOL送金完了: {sig}");

    Ok(())
}

/// TEEエンドポイントを直接呼び出す。
///
/// 管理コマンド（/register-node, /create-tree）はオペレーターが
/// TEEと同一ホスト上で実行するため、Gateway経由は不要。
/// TEEはネットワーク層（Security Group / vsock）で外部から隔離されている。
pub async fn call_tee_endpoint<T: serde::de::DeserializeOwned>(
    tee_url: &str,
    path: &str,
    request: &impl serde::Serialize,
) -> Result<Option<T>, CliError> {
    let client = reqwest::Client::new();
    let url = format!("{tee_url}{path}");
    println!("  {path}: {url}");

    match client
        .post(&url)
        .json(request)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                let result: T = resp.json().await?;
                Ok(Some(result))
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                println!(
                    "    HTTP {}: {}",
                    status,
                    &body[..body.len().min(100)]
                );
                Ok(None)
            }
        }
        Err(e) => {
            let msg = e.to_string();
            println!("    接続失敗: {}", &msg[..msg.len().min(60)]);
            Ok(None)
        }
    }
}
