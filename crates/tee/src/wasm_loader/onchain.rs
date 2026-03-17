// SPDX-License-Identifier: Apache-2.0

//! # オンチェーン WASMローダー
//!
//! 仕様書 §7.1, §7.3
//!
//! WasmModuleAccount PDAからwasm_source URLを読み取り、
//! Arweave等からWASMバイナリを取得する。
//!
//! 通信は既存の `proxy_client::proxy_post` / `proxy_get` を経由し、
//! vsockアーキテクチャに追加変更なく動作する。

use std::future::Future;
use std::pin::Pin;

use super::WasmBinary;
use super::WasmLoader;

/// オンチェーンPDAからWASM URLを解決し、Arweaveから取得するローダー。
///
/// 1. extension_idからWasmModuleAccount PDAアドレスを導出
/// 2. `proxy_post` でSolana RPC getAccountInfo → PDAデータ取得
/// 3. 最新activeバージョンのwasm_source URLを抽出
/// 4. `proxy_get` でそのURLからWASMバイナリ取得
pub struct OnChainLoader {
    /// Solana RPC URL
    rpc_url: String,
    /// title-configプログラムID
    program_id: [u8; 32],
    /// プロキシアドレス（"127.0.0.1:8000" or "direct"）
    proxy_addr: String,
}

impl OnChainLoader {
    pub fn new(rpc_url: String, program_id: [u8; 32], proxy_addr: String) -> Self {
        Self {
            rpc_url,
            program_id,
            proxy_addr,
        }
    }
}

/// extension_id文字列を32バイトにパディング。
fn extension_id_bytes(id: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let bytes = id.as_bytes();
    let len = bytes.len().min(32);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

/// WasmModuleAccount PDAアドレスを導出する。
/// seeds = [b"wasm-module", &extension_id]
fn find_wasm_module_pda(extension_id: &[u8; 32], program_id: &[u8; 32]) -> [u8; 32] {
    use ed25519_dalek::VerifyingKey;
    use sha2::{Digest, Sha256};

    for bump in (0..=255u8).rev() {
        let mut hasher = Sha256::new();
        hasher.update(b"wasm-module");
        hasher.update(extension_id);
        hasher.update([bump]);
        hasher.update(program_id);
        hasher.update(b"ProgramDerivedAddress");
        let hash: [u8; 32] = hasher.finalize().into();

        // カーブ外 = 有効なPDA
        if VerifyingKey::from_bytes(&hash).is_err() {
            return hash;
        }
    }
    [0u8; 32]
}

impl WasmLoader for OnChainLoader {
    fn load<'a>(
        &'a self,
        extension_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<WasmBinary, String>> + Send + 'a>> {
        Box::pin(async move {
            let ext_bytes = extension_id_bytes(extension_id);
            let pda_bytes = find_wasm_module_pda(&ext_bytes, &self.program_id);
            let pda_b58 = base58::ToBase58::to_base58(pda_bytes.as_slice());

            // Step 1: proxy_post で Solana RPC getAccountInfo
            let rpc_body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getAccountInfo",
                "params": [pda_b58, {"encoding": "base64"}]
            });
            let rpc_bytes = serde_json::to_vec(&rpc_body)
                .map_err(|e| format!("RPCリクエストのシリアライズに失敗: {e}"))?;

            let rpc_response = crate::infra::proxy_client::proxy_post(
                &self.proxy_addr,
                &self.rpc_url,
                &rpc_bytes,
            )
            .await
            .map_err(|e| format!("RPC通信に失敗: {e}"))?;

            if rpc_response.status != 200 {
                return Err(format!(
                    "RPC HTTPエラー: {} (extension_id: {extension_id})",
                    rpc_response.status
                ));
            }

            // Step 2: JSONパース → PDAデータ取得
            let rpc_json: serde_json::Value = serde_json::from_slice(&rpc_response.body)
                .map_err(|e| format!("RPC応答のパースに失敗: {e}"))?;

            let data_array = rpc_json["result"]["value"]["data"]
                .as_array()
                .ok_or_else(|| {
                    format!("WasmModuleAccount PDA が見つかりません (extension_id: {extension_id})")
                })?;

            let b64_str = data_array[0]
                .as_str()
                .ok_or("base64データが見つかりません")?;

            use base64::Engine;
            let account_data = base64::engine::general_purpose::STANDARD
                .decode(b64_str)
                .map_err(|e| format!("base64デコードに失敗: {e}"))?;

            // Step 3: PDAデータから最新activeバージョンのwasm_source URLを抽出
            let wasm_source = parse_latest_wasm_source(&account_data)
                .ok_or_else(|| {
                    format!("WasmModuleAccountにactiveバージョンがありません (extension_id: {extension_id})")
                })?;

            if wasm_source.is_empty() {
                return Err(format!(
                    "wasm_sourceが空です (extension_id: {extension_id})"
                ));
            }

            // Step 4: proxy_get で Arweave から WASM バイナリ取得
            tracing::info!(
                url = %wasm_source,
                extension_id = %extension_id,
                "WASMをArweaveから取得中"
            );

            let wasm_response = crate::infra::proxy_client::proxy_get(
                &self.proxy_addr,
                &wasm_source,
            )
            .await
            .map_err(|e| format!("WASM取得に失敗 ({wasm_source}): {e}"))?;

            if wasm_response.status != 200 {
                return Err(format!(
                    "WASM取得でHTTPエラー: {} ({wasm_source})",
                    wasm_response.status
                ));
            }

            if wasm_response.body.is_empty() {
                return Err(format!("WASM取得: 空のレスポンス ({wasm_source})"));
            }

            Ok(WasmBinary {
                source: wasm_source,
                bytes: wasm_response.body,
            })
        })
    }
}

/// WasmModuleAccountバイナリから最新activeバージョンのwasm_sourceを抽出する。
///
/// レイアウト:
///   discriminator(8) + extension_id(32) + Vec<WasmVersionEntry> + bump(1)
///
/// WasmVersionEntry:
///   version(u32) + wasm_hash([u8;32]) + wasm_source(String) + status(u8) + registered_at(i64)
fn parse_latest_wasm_source(data: &[u8]) -> Option<String> {
    if data.len() < 44 {
        return None;
    }
    let mut pos = 40; // skip discriminator(8) + extension_id(32)

    // Vec length
    if pos + 4 > data.len() {
        return None;
    }
    let n = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    pos += 4;

    let mut latest_active_source: Option<String> = None;

    for _ in 0..n {
        // version: u32
        if pos + 4 > data.len() {
            break;
        }
        pos += 4;

        // wasm_hash: [u8; 32]
        if pos + 32 > data.len() {
            break;
        }
        pos += 32;

        // wasm_source: String (Borsh: 4-byte len + bytes)
        if pos + 4 > data.len() {
            break;
        }
        let str_len = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
        pos += 4;
        if pos + str_len > data.len() {
            break;
        }
        let source = std::str::from_utf8(&data[pos..pos + str_len])
            .unwrap_or("")
            .to_string();
        pos += str_len;

        // status: u8 (0=active)
        if pos >= data.len() {
            break;
        }
        let status = data[pos];
        pos += 1;

        // registered_at: i64
        if pos + 8 > data.len() {
            break;
        }
        pos += 8;

        if status == 0 {
            latest_active_source = Some(source);
        }
    }

    latest_active_source
}
