// SPDX-License-Identifier: Apache-2.0

//! Title Protocol CLI。
//!
//! JSインフラスクリプトを統合したRust CLIバイナリ。
//! 3つのサブコマンド: init-global, register-node, create-tree

mod anchor;
mod commands;
mod config;
mod error;
mod helpers;
mod rpc;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "title-cli", about = "Title Protocol CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// GlobalConfig PDAの初期化、MPL Coreコレクション作成、WASMモジュール登録
    InitGlobal {
        /// Solana cluster (devnet / mainnet)
        #[arg(long, default_value = "devnet")]
        cluster: String,
        /// Solana RPC URL (省略時: cluster に応じたデフォルト)
        #[arg(long)]
        rpc: Option<String>,
        /// title-config プログラムID (省略時: デフォルト)
        #[arg(long)]
        program_id: Option<String>,
    },
    /// TEEノードのオンチェーン登録
    RegisterNode {
        /// TEE サーバーURL
        #[arg(long, default_value = "http://localhost:4000")]
        tee_url: String,
        /// Gateway 外部公開エンドポイント
        #[arg(long, default_value = "http://localhost:3000")]
        gateway_endpoint: String,
        /// TEE測定値 (JSON文字列, 例: '{"PCR0":"abcd..."}')
        #[arg(long)]
        measurements: Option<String>,
    },
    /// Core + Extension Merkle Tree の作成
    CreateTree {
        /// TEE サーバーURL
        #[arg(long, default_value = "http://localhost:4000")]
        tee_url: String,
        /// Merkle Treeの深さ
        #[arg(long, default_value = "14")]
        max_depth: u32,
        /// 最大バッファサイズ
        #[arg(long, default_value = "64")]
        max_buffer_size: u32,
    },
}

/// プロジェクトルートを検出する。
/// Cargo.toml が存在するディレクトリを上方探索する。
fn find_project_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("crates").exists() {
            return dir;
        }
        if !dir.pop() {
            // フォールバック: カレントディレクトリ
            return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let project_root = find_project_root();

    let result = match cli.command {
        Commands::InitGlobal {
            cluster,
            rpc,
            program_id,
        } => {
            commands::init_global::run(
                &project_root,
                &cluster,
                rpc.as_deref(),
                program_id.as_deref(),
            )
            .await
        }
        Commands::RegisterNode {
            tee_url,
            gateway_endpoint,
            measurements,
        } => {
            commands::register_node::run(
                &project_root,
                &tee_url,
                &gateway_endpoint,
                measurements.as_deref(),
            )
            .await
        }
        Commands::CreateTree {
            tee_url,
            max_depth,
            max_buffer_size,
        } => {
            commands::create_tree::run(&project_root, &tee_url, max_depth, max_buffer_size)
                .await
        }
    };

    if let Err(e) = result {
        eprintln!("\nFATAL: {e}");
        std::process::exit(1);
    }
}
