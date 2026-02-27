// SPDX-License-Identifier: Apache-2.0

//! `title-cli register-node` サブコマンド。
//!
//! TEEノードのオンチェーン登録を行う。
//! TEEが部分署名したTXに対し、authority keypairがあれば自動署名+ブロードキャスト、
//! なければDAO承認用の部分署名TXを表示する。

use std::path::Path;

use base64::Engine;
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey,
    signature::Signature,
    signer::Signer,
    transaction::Transaction,
};

use title_types::{RegisterNodeRequest, RegisterNodeResponse};

use crate::config;
use crate::error::CliError;
use crate::helpers;
use crate::rpc::{b64, SolanaRpc};

/// register-node サブコマンドを実行する。
#[allow(deprecated)]
pub async fn run(
    project_root: &Path,
    tee_url: &str,
    gateway_endpoint: &str,
    measurements_json: Option<&str>,
) -> Result<(), CliError> {
    println!("[register-node] TEEノード登録...");

    // network.json 読み込み
    let network_path = project_root.join("network.json");
    let network = config::load_network_config(&network_path)?;
    let rpc_url = config::resolve_rpc_url(&network.cluster, None);
    let rpc = SolanaRpc::new(&rpc_url);

    // Gateway公開鍵の導出（GATEWAY_SIGNING_KEYから）
    let gateway_pubkey = derive_gateway_pubkey()?;
    println!("  Gateway pubkey: {gateway_pubkey}");

    // measurements のパース
    let measurements: std::collections::HashMap<String, String> =
        if let Some(json_str) = measurements_json {
            serde_json::from_str(json_str).unwrap_or_default()
        } else {
            Default::default()
        };

    // Blockhash取得
    let blockhash = rpc.get_latest_blockhash().await?;

    // /register-node リクエスト
    let register_request = RegisterNodeRequest {
        gateway_endpoint: gateway_endpoint.to_string(),
        gateway_pubkey: gateway_pubkey.clone(),
        recent_blockhash: blockhash.to_string(),
        authority: network.authority.clone(),
        program_id: network.program_id.clone(),
        measurements,
    };

    let result: RegisterNodeResponse =
        match helpers::call_tee_endpoint(tee_url, "/register-node", &register_request).await? {
            Some(r) => r,
            None => {
                println!("  WARNING: TEEに接続できません。TEE起動後に再実行してください。");
                return Ok(());
            }
        };

    println!("  TEE Signing Pubkey: {}", result.signing_pubkey);
    println!("  TEE Node PDA: {}", result.tee_node_pda);

    // TEE walletにSOL送金（TX手数料用）
    let tee_pk: Pubkey = result
        .signing_pubkey
        .parse()
        .map_err(|e| CliError::Config(format!("signing_pubkeyのパースに失敗: {e}")))?;

    helpers::fund_tee_wallet(&rpc, project_root, &tee_pk, 100_000_000).await?;

    // Authority keypair の存在で分岐
    let authority_key_path = project_root
        .join("programs")
        .join("title-config")
        .join("keys")
        .join("authority.json");
    let tx_bytes = b64()
        .decode(&result.partial_tx)
        .map_err(|e| CliError::Config(format!("partial_txのデコードに失敗: {e}")))?;

    if authority_key_path.exists() {
        // devnet: authority keypairがローカルにある → 自分で署名+ブロードキャスト
        println!("  Authority keypair 検出 → 自動署名");
        let authority = config::load_keypair(&authority_key_path)?
            .ok_or_else(|| CliError::Config("Authority keypairのロードに失敗".into()))?;

        let mut reg_tx: Transaction = bincode::deserialize(&tx_bytes)
            .map_err(|e| CliError::Transaction(format!("TXのデシリアライズに失敗: {e}")))?;

        // Authority部分署名を適用
        let message_bytes = reg_tx.message.serialize();
        let authority_sig = authority.try_sign_message(&message_bytes)
            .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

        apply_partial_signature(&mut reg_tx, &authority.pubkey(), &authority_sig)?;

        let signed_bytes = bincode::serialize(&reg_tx)
            .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

        match rpc.send_and_confirm(&signed_bytes).await {
            Ok(sig) => println!("  TEEノード登録完了: {sig}"),
            Err(e) => println!(
                "  TEEノード登録失敗（既に登録済みの可能性）: {e}"
            ),
        }
    } else {
        // mainnet: DAOのガバナンスシステムに審査依頼
        let tx_base64 = b64().encode(&tx_bytes);
        println!();
        println!("  === DAO承認が必要 ===");
        println!("  Authority keypair が見つかりません。");
        println!("  以下の部分署名済みTXをDAOに提出してください:");
        println!();
        println!("  TEE Signing Pubkey: {}", result.signing_pubkey);
        println!("  TEE Node PDA:       {}", result.tee_node_pda);
        println!("  Partial TX (base64):");
        println!("  {tx_base64}");
        println!();
        println!("  DAOが共同署名 → Solanaにブロードキャストすれば登録完了");
        println!("  ========================");
    }

    // tee-info.json を保存
    let tee_info_path = project_root
        .join("tests")
        .join("e2e")
        .join("fixtures")
        .join("tee-info.json");
    let mut info = config::load_tee_info(&tee_info_path)?;
    info.signing_pubkey = Some(result.signing_pubkey);
    info.encryption_pubkey = Some(result.encryption_pubkey);
    info.tee_node_pda = Some(result.tee_node_pda);
    config::save_tee_info(&tee_info_path, &info)?;
    println!("  TEE情報を保存: {}", tee_info_path.display());

    Ok(())
}

/// GATEWAY_SIGNING_KEY環境変数からEd25519公開鍵を導出する。
fn derive_gateway_pubkey() -> Result<String, CliError> {
    let hex_key = std::env::var("GATEWAY_SIGNING_KEY").map_err(|_| {
        CliError::Config("GATEWAY_SIGNING_KEY 環境変数が設定されていません".into())
    })?;

    let seed_bytes = hex::decode(&hex_key).map_err(|e| {
        CliError::Config(format!("GATEWAY_SIGNING_KEY のHexデコードに失敗: {e}"))
    })?;

    if seed_bytes.len() < 32 {
        return Err(CliError::Config(
            "GATEWAY_SIGNING_KEY は32バイト以上必要です".into(),
        ));
    }

    let secret =
        ed25519_dalek::SigningKey::from_bytes(seed_bytes[..32].try_into().unwrap());
    let pubkey_bytes = secret.verifying_key().to_bytes();
    let solana_pubkey = Pubkey::new_from_array(pubkey_bytes);
    Ok(solana_pubkey.to_string())
}

/// トランザクションに部分署名を適用する。
fn apply_partial_signature(
    tx: &mut Transaction,
    pubkey: &Pubkey,
    signature: &Signature,
) -> Result<(), CliError> {
    let num_signers = tx.message.header.num_required_signatures as usize;
    for (i, key) in tx.message.account_keys.iter().enumerate() {
        if i >= num_signers {
            break;
        }
        if key == pubkey {
            tx.signatures[i] = *signature;
            return Ok(());
        }
    }
    Err(CliError::Transaction(format!(
        "公開鍵 {pubkey} がトランザクションの署名者に見つかりません"
    )))
}
