// SPDX-License-Identifier: Apache-2.0

//! Title Protocol CLI — Gateway を操作するコマンドラインツール。

use clap::{Parser, Subcommand};

mod client;
mod solana;
mod whitelist;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";

#[derive(Parser)]
#[command(name = "title-cli", about = "Title Protocol CLI")]
struct Cli {
    /// Gateway endpoint URL
    #[arg(long, env = "GATEWAY_URL", default_value = "http://localhost:3000")]
    gateway: String,

    /// API key for authenticated endpoints
    #[arg(long, env = "API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stack の状態を確認
    Health,

    /// TEE の暗号化公開鍵を表示
    Keys,

    /// 登録済み processor 一覧を表示
    Processors,

    /// Solana 署名用公開鍵を表示
    SolanaKeys,

    /// C2PA 署名付きコンテンツを処理
    Process {
        /// コンテンツ URL (single 入力)
        #[arg(long)]
        url: String,

        /// 実行する processor ID (カンマ区切り)
        #[arg(long, default_value = "c2pa-verify")]
        processors: String,
    },

    /// サイドカー形式のコンテンツを処理
    ProcessSidecar {
        /// C2PA マニフェスト (.c2pa) の URL
        #[arg(long)]
        manifest_url: String,

        /// コンテンツファイルの URL
        #[arg(long)]
        content_url: String,

        /// 実行する processor ID (カンマ区切り)
        #[arg(long, default_value = "c2pa-verify")]
        processors: String,
    },

    /// Bubblegum V2 Merkle tree を作成
    CreateTree {
        /// Payer keypair JSON ファイルパス
        #[arg(long)]
        payer: String,

        /// Merkle tree の最大深度
        #[arg(long, default_value = "14")]
        max_depth: u32,

        /// Merkle tree のバッファサイズ
        #[arg(long, default_value = "64")]
        max_buffer_size: u32,

        /// Solana RPC URL
        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// cNFT をミント（処理結果 → Solana Extension → broadcast）
    Mint {
        /// オフチェーンデータ URL（ProcessResponse JSON）
        #[arg(long)]
        offchain_data_url: String,

        /// Merkle tree アドレス (Base58)
        #[arg(long)]
        merkle_tree: String,

        /// Payer keypair JSON ファイルパス
        #[arg(long)]
        payer: String,

        /// コレクションアドレス (Base58, 任意)
        #[arg(long)]
        collection: Option<String>,

        /// Solana RPC URL
        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// 許可レジストリ (ApprovedVkeys + ApprovedMeasurements) を初期化
    InitRegistries {
        /// Admin keypair JSON ファイルパス
        #[arg(long)]
        admin: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// SP1 verifying-key hash を許可リストに追加
    AddVkey {
        /// Admin keypair JSON ファイルパス
        #[arg(long)]
        admin: String,

        /// vkey_hash 32 バイト (hex, 0x プレフィックス任意)
        #[arg(long)]
        vkey_hex: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// SP1 verifying-key hash を許可リストから削除
    RemoveVkey {
        #[arg(long)]
        admin: String,

        #[arg(long)]
        vkey_hex: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// TEE measurement (e.g. AWS Nitro PCR0, 48 バイト) を許可リストに追加
    AddMeasurement {
        #[arg(long)]
        admin: String,

        /// PCR0 hex (0x プレフィックス任意)
        #[arg(long)]
        pcr0_hex: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// TEE measurement を許可リストから削除
    RemoveMeasurement {
        #[arg(long)]
        admin: String,

        #[arg(long)]
        pcr0_hex: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// TEE 署名鍵を on-chain ホワイトリストに登録（register_key 提出）
    ///
    /// --bundle <dir> 配下に以下のファイルが揃っている必要がある:
    ///   solana_pubkey.txt
    ///   attestation.bin.proof.bin
    ///   attestation.bin.public_values.bin
    ///   attestation.bin.vkey_hash.hex
    RegisterKey {
        /// Payer keypair JSON ファイルパス（admin である必要はない）
        #[arg(long)]
        payer: String,

        /// fetch-registration-bundle.sh と prove の出力を集約したディレクトリ
        #[arg(long)]
        bundle: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// 登録済み TEE 署名鍵を取り消す（PDA は close せず revoked フラグを立てる）
    RevokeKey {
        #[arg(long)]
        admin: String,

        /// 取り消す signing pubkey (Base58)
        #[arg(long)]
        signing_pubkey: String,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },

    /// 許可レジストリと WhitelistEntry の状態を表示
    DescribeWhitelist {
        /// 任意: 指定の signing pubkey (Base58) について WhitelistEntry も読む
        #[arg(long)]
        signing_pubkey: Option<String>,

        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEVNET_RPC)]
        rpc_url: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let gw = client::GatewayClient::new(&cli.gateway, cli.api_key.as_deref());

    let result = match cli.command {
        Command::Health => gw.health(),
        Command::Keys => gw.get_json("/keys"),
        Command::Processors => gw.get_json("/processors"),
        Command::SolanaKeys => gw.get_json("/solana-keys"),
        Command::Process { url, processors } => {
            let ids: Vec<String> = processors.split(',').map(|s| s.trim().to_string()).collect();
            gw.process_single(&url, &ids)
        }
        Command::ProcessSidecar {
            manifest_url,
            content_url,
            processors,
        } => {
            let ids: Vec<String> = processors.split(',').map(|s| s.trim().to_string()).collect();
            gw.process_sidecar(&manifest_url, &content_url, &ids)
        }
        Command::CreateTree {
            payer,
            max_depth,
            max_buffer_size,
            rpc_url,
        } => cmd_create_tree(&gw, &payer, max_depth, max_buffer_size, &rpc_url),
        Command::Mint {
            offchain_data_url,
            merkle_tree,
            payer,
            collection,
            rpc_url,
        } => cmd_mint(
            &gw,
            &offchain_data_url,
            &merkle_tree,
            &payer,
            collection.as_deref(),
            &rpc_url,
        ),
        Command::InitRegistries { admin, rpc_url } => {
            whitelist::cmd_init_registries(&admin, &rpc_url)
        }
        Command::AddVkey {
            admin,
            vkey_hex,
            rpc_url,
        } => whitelist::cmd_add_vkey(&admin, &vkey_hex, &rpc_url),
        Command::RemoveVkey {
            admin,
            vkey_hex,
            rpc_url,
        } => whitelist::cmd_remove_vkey(&admin, &vkey_hex, &rpc_url),
        Command::AddMeasurement {
            admin,
            pcr0_hex,
            rpc_url,
        } => whitelist::cmd_add_measurement(&admin, &pcr0_hex, &rpc_url),
        Command::RemoveMeasurement {
            admin,
            pcr0_hex,
            rpc_url,
        } => whitelist::cmd_remove_measurement(&admin, &pcr0_hex, &rpc_url),
        Command::RegisterKey {
            payer,
            bundle,
            rpc_url,
        } => whitelist::cmd_register_key(&payer, &bundle, &rpc_url),
        Command::RevokeKey {
            admin,
            signing_pubkey,
            rpc_url,
        } => whitelist::cmd_revoke_key(&admin, &signing_pubkey, &rpc_url),
        Command::DescribeWhitelist {
            signing_pubkey,
            rpc_url,
        } => whitelist::cmd_describe(&rpc_url, signing_pubkey.as_deref()),
    };

    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_create_tree(
    gw: &client::GatewayClient,
    payer_path: &str,
    max_depth: u32,
    max_buffer_size: u32,
    rpc_url: &str,
) -> Result<(), String> {
    let payer_key = solana::load_keypair(payer_path)?;
    let payer_pubkey = solana::pubkey(&payer_key);

    println!("payer:    {payer_pubkey}");
    println!("rpc:      {rpc_url}");

    let blockhash = solana::get_blockhash(rpc_url)?;
    println!("blockhash: {blockhash}");

    let resp = gw.create_tree(
        &payer_pubkey.to_string(),
        max_depth,
        max_buffer_size,
        &blockhash.to_string(),
    )?;

    let partial_tx_b64 = resp["partial_tx"]
        .as_str()
        .ok_or("missing partial_tx in response")?;
    let tree_pubkey = resp["tree_pubkey"]
        .as_str()
        .ok_or("missing tree_pubkey in response")?;

    println!("tree:     {tree_pubkey}");

    let mut tx = solana::decode_partial_tx(partial_tx_b64)?;
    solana::sign_transaction(&mut tx, &payer_key)?;

    let sig = solana::broadcast(rpc_url, &tx)?;
    println!("tx:       {sig}");
    println!("confirmed");

    Ok(())
}

fn cmd_mint(
    gw: &client::GatewayClient,
    offchain_data_url: &str,
    merkle_tree: &str,
    payer_path: &str,
    collection: Option<&str>,
    rpc_url: &str,
) -> Result<(), String> {
    let payer_key = solana::load_keypair(payer_path)?;
    let payer_pubkey = solana::pubkey(&payer_key);

    println!("payer:    {payer_pubkey}");
    println!("tree:     {merkle_tree}");
    println!("rpc:      {rpc_url}");

    let blockhash = solana::get_blockhash(rpc_url)?;
    println!("blockhash: {blockhash}");

    let resp = gw.solana_extension(
        offchain_data_url,
        &payer_pubkey.to_string(),
        merkle_tree,
        &blockhash.to_string(),
        collection,
    )?;

    let partial_tx_b64 = resp["partial_tx"]
        .as_str()
        .ok_or("missing partial_tx in response")?;

    let mut tx = solana::decode_partial_tx(partial_tx_b64)?;
    solana::sign_transaction(&mut tx, &payer_key)?;

    let sig = solana::broadcast(rpc_url, &tx)?;
    println!("tx:       {sig}");
    println!("confirmed");

    Ok(())
}
