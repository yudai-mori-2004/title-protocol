// SPDX-License-Identifier: Apache-2.0

//! 設定ファイルI/O（network.json, keypair, tee-info.json）。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
#[allow(deprecated)]
use solana_sdk::signer::keypair::Keypair;

use crate::error::CliError;

/// network.json の構造。init-global が生成し、他コマンドが参照する。
#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub cluster: String,
    pub program_id: String,
    pub global_config_pda: String,
    pub authority: String,
    pub core_collection_mint: String,
    pub ext_collection_mint: String,
    pub wasm_modules: HashMap<String, WasmModuleInfo>,
}

/// WASM モジュール情報（network.json 内）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmModuleInfo {
    pub hash: String,
}

/// TEE情報（tests/e2e/fixtures/tee-info.json）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TeeInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tee_node_pda: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_tree_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext_tree_address: Option<String>,
}

/// Solana CLI形式 (JSON array of u8) のキーペアをロード。
/// 存在しなければ新規生成して保存する。
#[allow(deprecated)]
pub fn load_or_create_authority(path: &Path) -> Result<Keypair, CliError> {
    if path.exists() {
        println!("  既存のキーペアをロード: {}", path.display());
        let raw = std::fs::read_to_string(path)?;
        let bytes: Vec<u8> = serde_json::from_str(&raw)?;
        Keypair::from_bytes(&bytes)
            .map_err(|e| CliError::Config(format!("キーペアのパースに失敗: {e}")))
    } else {
        println!("  新しいキーペアを生成中...");
        let kp = Keypair::new();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes: Vec<u8> = kp.to_bytes().to_vec();
        std::fs::write(path, serde_json::to_string(&bytes)?)?;
        println!("  保存先: {}", path.display());
        Ok(kp)
    }
}

/// Solana CLI形式のキーペアをロード（存在しない場合はNone）。
#[allow(deprecated)]
pub fn load_keypair(path: &Path) -> Result<Option<Keypair>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let bytes: Vec<u8> = serde_json::from_str(&raw)?;
    let kp = Keypair::from_bytes(&bytes)
        .map_err(|e| CliError::Config(format!("キーペアのパースに失敗: {e}")))?;
    Ok(Some(kp))
}

/// network.json の読み込み。
pub fn load_network_config(path: &Path) -> Result<NetworkConfig, CliError> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// network.json の書き出し。
pub fn save_network_config(path: &Path, config: &NetworkConfig) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(config)? + "\n";
    std::fs::write(path, json)?;
    Ok(())
}

/// tee-info.json の読み込み（存在しなければデフォルト）。
pub fn load_tee_info(path: &Path) -> Result<TeeInfo, CliError> {
    if !path.exists() {
        return Ok(TeeInfo::default());
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// tee-info.json の書き出し。
pub fn save_tee_info(path: &Path, info: &TeeInfo) -> Result<(), CliError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(info)? + "\n";
    std::fs::write(path, json)?;
    Ok(())
}

/// cluster名からデフォルトRPC URLを解決する。
pub fn resolve_rpc_url(cluster: &str, rpc_override: Option<&str>) -> String {
    if let Some(rpc) = rpc_override {
        return rpc.to_string();
    }
    if let Ok(env_url) = std::env::var("SOLANA_RPC_URL") {
        if !env_url.is_empty() {
            return env_url;
        }
    }
    match cluster {
        "mainnet" => "https://api.mainnet-beta.solana.com".to_string(),
        _ => "https://api.devnet.solana.com".to_string(),
    }
}
