// SPDX-License-Identifier: Apache-2.0

//! `title-cli init-global` サブコマンド。
//!
//! GlobalConfig PDA の初期化、MPL Core コレクション作成、WASM モジュール登録を行う。
//! 実行後 `network.json` をプロジェクトルートに書き出す。

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
use crate::config::{
    self, NetworkConfig, WasmModuleInfo,
};
use crate::error::CliError;
use crate::rpc::SolanaRpc;

/// デフォルトプログラムID。
const DEFAULT_PROGRAM_ID: &str = "GXo7dQ4kW8oeSSSK2Lhaw1jakNps1fSeUHEfeb7dRsYP";

/// WASM モジュールID一覧。
const WASM_MODULES: &[&str] = &[
    "phash-v1",
    "hardware-google",
    "c2pa-training-v1",
    "c2pa-license-v1",
];

/// init-global サブコマンドを実行する。
#[allow(deprecated)]
pub async fn run(
    project_root: &Path,
    cluster: &str,
    rpc_override: Option<&str>,
    program_id_override: Option<&str>,
) -> Result<(), CliError> {
    let rpc_url = config::resolve_rpc_url(cluster, rpc_override);
    let program_id: Pubkey = program_id_override
        .unwrap_or(DEFAULT_PROGRAM_ID)
        .parse()
        .map_err(|e| CliError::Config(format!("program_idのパースに失敗: {e}")))?;

    println!("=== Title Protocol GlobalConfig 初期化 ===\n");
    println!("  Cluster: {cluster}");
    println!("  RPC: {rpc_url}");
    println!("  Program: {program_id}\n");

    let rpc = SolanaRpc::new(&rpc_url);

    // =====================================================================
    // Step 1: Authority Keypair
    // =====================================================================
    println!("[Step 1] Authority Keypair");
    let authority_path = project_root
        .join("programs")
        .join("title-config")
        .join("keys")
        .join("authority.json");
    let authority = config::load_or_create_authority(&authority_path)?;
    let authority_pubkey = authority.pubkey();
    println!("  Authority: {authority_pubkey}");

    // =====================================================================
    // Step 2: Airdrop (devnetのみ)
    // =====================================================================
    if cluster == "devnet" {
        println!("\n[Step 2] Airdrop (devnet)");
        let balance = rpc.get_balance(&authority_pubkey).await?;
        let balance_sol = balance as f64 / 1_000_000_000.0;
        if balance_sol < 2.0 {
            println!("  残高 {balance_sol:.4} SOL → Airdrop中...");
            match rpc
                .request_airdrop(&authority_pubkey, 2_000_000_000)
                .await
            {
                Ok(sig) => {
                    if let Err(e) = rpc.confirm_transaction(&sig).await {
                        println!("  Airdrop確認失敗: {e}");
                    } else {
                        println!("  Airdrop完了 (+2 SOL)");
                    }
                }
                Err(e) => {
                    println!("  Airdrop失敗: {e}");
                    if balance_sol < 0.01 {
                        eprintln!("  ERROR: SOL残高不足。手動でairdropしてください:");
                        eprintln!(
                            "    solana airdrop 2 {authority_pubkey} --url {rpc_url}"
                        );
                        return Err(CliError::Config("SOL残高不足".into()));
                    }
                }
            }
        } else {
            println!("  残高: {balance_sol:.4} SOL (十分)");
        }
    } else {
        println!("\n[Step 2] Airdrop → スキップ (mainnet)");
        let balance = rpc.get_balance(&authority_pubkey).await?;
        let balance_sol = balance as f64 / 1_000_000_000.0;
        println!("  残高: {balance_sol:.4} SOL");
        if balance < 100_000_000 {
            eprintln!("  ERROR: SOL残高不足です。事前に送金してください。");
            return Err(CliError::Config("SOL残高不足".into()));
        }
    }

    // =====================================================================
    // Step 3: MPL Core Collections + GlobalConfig
    // =====================================================================
    println!("\n[Step 3] MPL Core コレクション作成");

    let (global_config_pda, _) = anchor::find_global_config_pda(&program_id);
    println!("  GlobalConfig PDA: {global_config_pda}");

    let existing_data = rpc.get_account_data(&global_config_pda).await?;

    let (core_mint_str, ext_mint_str) = if let Some(data) = existing_data {
        handle_existing_config(
            &rpc,
            &data,
            &program_id,
            &global_config_pda,
            &authority,
        )
        .await?
    } else {
        handle_new_config(
            &rpc,
            &program_id,
            &global_config_pda,
            &authority,
        )
        .await?
    };

    // =====================================================================
    // Step 5: WASM モジュール登録
    // =====================================================================
    println!("\n[Step 5] WASM モジュール登録");

    let mut wasm_module_info = std::collections::HashMap::new();

    for module_id in WASM_MODULES {
        let wasm_filename = module_id.replace('-', "_") + ".wasm";
        let local_path = project_root
            .join("wasm")
            .join(module_id)
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("release")
            .join(&wasm_filename);

        if !local_path.exists() {
            println!("  {module_id}: ローカルビルドなし → スキップ");
            println!(
                "    ビルド: cd wasm/{module_id} && cargo build --target wasm32-unknown-unknown --release"
            );
            continue;
        }

        let wasm_bytes = std::fs::read(&local_path)?;
        let hash: [u8; 32] = Sha256::digest(&wasm_bytes).into();
        let hash_hex = hex::encode(hash);
        println!(
            "  {module_id}: {}... ({} bytes)",
            &hash_hex[..16],
            wasm_bytes.len()
        );

        let ix = anchor::build_add_wasm_module_ix(
            &program_id,
            &global_config_pda,
            &authority_pubkey,
            module_id,
            &hash,
            "",
        );

        let blockhash = rpc.get_latest_blockhash().await?;
        let message =
            Message::new_with_blockhash(&[ix], Some(&authority_pubkey), &blockhash);
        let mut tx = Transaction::new_unsigned(message);
        tx.try_sign(&[&authority], blockhash)
            .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

        let tx_bytes = bincode::serialize(&tx)
            .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;

        match rpc.send_and_confirm(&tx_bytes).await {
            Ok(sig) => println!("    登録完了: {sig}"),
            Err(e) => println!("    登録失敗: {e}"),
        }

        wasm_module_info.insert(
            module_id.to_string(),
            WasmModuleInfo { hash: hash_hex },
        );
    }

    // =====================================================================
    // Step 6: network.json 書き出し
    // =====================================================================
    println!("\n[Step 6] network.json 書き出し");

    let network_config = NetworkConfig {
        cluster: cluster.to_string(),
        program_id: program_id.to_string(),
        global_config_pda: global_config_pda.to_string(),
        authority: authority_pubkey.to_string(),
        core_collection_mint: core_mint_str.clone(),
        ext_collection_mint: ext_mint_str.clone(),
        wasm_modules: wasm_module_info.clone(),
    };

    let network_path = project_root.join("network.json");
    config::save_network_config(&network_path, &network_config)?;
    println!("  保存先: {}", network_path.display());

    // =====================================================================
    // サマリー
    // =====================================================================
    println!("\n=== GlobalConfig 初期化完了 ===");
    println!("  Cluster:              {cluster}");
    println!("  Authority:            {authority_pubkey}");
    println!("  Authority keypair:    {}", authority_path.display());
    println!("  GlobalConfig PDA:     {global_config_pda}");
    println!("  Core Collection:      {core_mint_str}");
    println!("  Extension Collection: {ext_mint_str}");
    println!("  Program ID:           {program_id}");
    println!(
        "  WASM Modules:         {}/{}",
        wasm_module_info.len(),
        WASM_MODULES.len()
    );
    println!("  network.json:         {}", network_path.display());
    println!();
    println!("次のステップ:");
    println!("  1. network.json をリポジトリにコミット");
    println!("  2. ノード起動: deploy/aws/setup-ec2.sh");
    if cluster == "devnet" {
        println!("  ※ programs/title-config/keys/authority.json を各ノードにコピーすれば自動承認可能");
    } else {
        println!("  ※ mainnet: ノード登録TXはDAO承認が必要");
    }
    println!();

    Ok(())
}

/// 既存の GlobalConfig がある場合の処理。
/// コレクションが有効ならスキップ、無効なら新規作成 → update_collections。
#[allow(deprecated)]
async fn handle_existing_config(
    rpc: &SolanaRpc,
    data: &[u8],
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    authority: &solana_sdk::signer::keypair::Keypair,
) -> Result<(String, String), CliError> {
    // Anchor形式: 8B discriminator + 32B authority + 32B core_mint + 32B ext_mint
    if data.len() < 8 + 32 + 32 + 32 {
        return Err(CliError::Config(
            "GlobalConfigデータが不正です".into(),
        ));
    }

    let core_mint_bytes: [u8; 32] = data[8 + 32..8 + 32 + 32]
        .try_into()
        .map_err(|_| CliError::Config("core_mintのパースに失敗".into()))?;
    let ext_mint_bytes: [u8; 32] = data[8 + 32 + 32..8 + 32 + 32 + 32]
        .try_into()
        .map_err(|_| CliError::Config("ext_mintのパースに失敗".into()))?;

    let core_mint = Pubkey::new_from_array(core_mint_bytes);
    let ext_mint = Pubkey::new_from_array(ext_mint_bytes);

    // コレクションアカウントの存在確認
    let core_acct = rpc.get_account_data(&core_mint).await?;
    let ext_acct = rpc.get_account_data(&ext_mint).await?;

    if core_acct.is_some() && ext_acct.is_some() {
        println!("  既存のコレクションを使用:");
        println!("    Core:      {core_mint}");
        println!("    Extension: {ext_mint}");
        return Ok((core_mint.to_string(), ext_mint.to_string()));
    }

    // コレクションが無効 → 新規作成 + update_collections
    println!("  コレクションが無効。新規作成 → update_collections で更新します。");

    let (core_mint_str, ext_mint_str) =
        create_collections(rpc, authority).await?;

    let new_core: Pubkey = core_mint_str
        .parse()
        .map_err(|e| CliError::Config(format!("core_mintのパースに失敗: {e}")))?;
    let new_ext: Pubkey = ext_mint_str
        .parse()
        .map_err(|e| CliError::Config(format!("ext_mintのパースに失敗: {e}")))?;

    let ix = anchor::build_update_collections_ix(
        program_id,
        global_config_pda,
        &authority.pubkey(),
        &new_core,
        &new_ext,
    );

    let blockhash = rpc.get_latest_blockhash().await?;
    let message =
        Message::new_with_blockhash(&[ix], Some(&authority.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[authority], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;
    let sig = rpc.send_and_confirm(&tx_bytes).await?;
    println!("  update_collections 完了: {sig}");

    Ok((core_mint_str, ext_mint_str))
}

/// GlobalConfig 未初期化の場合の処理。
/// コレクション作成 → initialize。
#[allow(deprecated)]
async fn handle_new_config(
    rpc: &SolanaRpc,
    program_id: &Pubkey,
    global_config_pda: &Pubkey,
    authority: &solana_sdk::signer::keypair::Keypair,
) -> Result<(String, String), CliError> {
    let (core_mint_str, ext_mint_str) =
        create_collections(rpc, authority).await?;

    println!("\n[Step 4] GlobalConfig 初期化");

    let core_mint: Pubkey = core_mint_str
        .parse()
        .map_err(|e| CliError::Config(format!("core_mintのパースに失敗: {e}")))?;
    let ext_mint: Pubkey = ext_mint_str
        .parse()
        .map_err(|e| CliError::Config(format!("ext_mintのパースに失敗: {e}")))?;

    let ix = anchor::build_initialize_ix(
        program_id,
        global_config_pda,
        &authority.pubkey(),
        &core_mint,
        &ext_mint,
    );

    let blockhash = rpc.get_latest_blockhash().await?;
    let message =
        Message::new_with_blockhash(&[ix], Some(&authority.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[authority], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;
    let sig = rpc.send_and_confirm(&tx_bytes).await?;
    println!("  GlobalConfig 初期化完了: {sig}");

    Ok((core_mint_str, ext_mint_str))
}

/// MPL Core コレクションを2つ（Core + Extension）作成する。
#[allow(deprecated)]
async fn create_collections(
    rpc: &SolanaRpc,
    authority: &solana_sdk::signer::keypair::Keypair,
) -> Result<(String, String), CliError> {
    use solana_sdk::signer::keypair::Keypair;

    println!("  Core Collection 作成中...");
    let core_collection = Keypair::new();
    let core_mint_str = create_one_collection(
        rpc,
        authority,
        &core_collection,
        "Title Protocol Core",
    )
    .await?;

    println!("  Extension Collection 作成中...");
    let ext_collection = Keypair::new();
    let ext_mint_str = create_one_collection(
        rpc,
        authority,
        &ext_collection,
        "Title Protocol Extension",
    )
    .await?;

    Ok((core_mint_str, ext_mint_str))
}

/// 1つのMPL Core コレクションを作成する。
#[allow(deprecated)]
async fn create_one_collection(
    rpc: &SolanaRpc,
    payer: &solana_sdk::signer::keypair::Keypair,
    collection_keypair: &solana_sdk::signer::keypair::Keypair,
    name: &str,
) -> Result<String, CliError> {
    let collection_pubkey = collection_keypair.pubkey();
    println!("    Collection address: {collection_pubkey}");

    let ix = anchor::build_create_collection_ix(
        &collection_pubkey,
        &payer.pubkey(),
        name,
        "",
    );

    let blockhash = rpc.get_latest_blockhash().await?;
    let message =
        Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[payer, collection_keypair], blockhash)
        .map_err(|e| CliError::Transaction(format!("署名に失敗: {e}")))?;

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| CliError::Transaction(format!("シリアライズに失敗: {e}")))?;
    let sig = rpc.send_and_confirm(&tx_bytes).await?;
    println!(
        "    作成完了 (sig: {}...)",
        &sig[..sig.len().min(20)]
    );

    Ok(collection_pubkey.to_string())
}
