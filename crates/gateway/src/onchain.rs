// SPDX-License-Identifier: Apache-2.0

//! # オンチェーン GlobalConfig 読み取り
//!
//! 仕様書 §6.2: Solana RPC経由でGlobalConfigアカウントからResourceLimitsを取得する。
//!
//! Borsh形式のアカウントデータを手動パースし、可変長フィールド（Vec）を
//! 読み飛ばしてResourceLimitsを抽出する。

use title_types::ResourceLimits;

/// GlobalConfig から ResourceLimits を読み取る。
///
/// Solana JSON-RPC `getAccountInfo` → base64デコード → Borsh手動パース。
/// 失敗した場合はNoneを返す（Gatewayはデフォルト値で動作する）。
pub async fn fetch_on_chain_resource_limits(
    client: &reqwest::Client,
    rpc_url: &str,
    global_config_pda: &str,
) -> Option<ResourceLimits> {
    let result = fetch_inner(client, rpc_url, global_config_pda).await;
    match result {
        Ok(limits) => Some(limits),
        Err(e) => {
            tracing::warn!("オンチェーンResourceLimits取得失敗: {e}");
            None
        }
    }
}

async fn fetch_inner(
    client: &reqwest::Client,
    rpc_url: &str,
    global_config_pda: &str,
) -> Result<ResourceLimits, Box<dyn std::error::Error>> {
    // Solana JSON-RPC: getAccountInfo
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAccountInfo",
        "params": [
            global_config_pda,
            { "encoding": "base64" }
        ]
    });

    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await?;

    let json: serde_json::Value = resp.json().await?;

    let data_arr = json["result"]["value"]["data"]
        .as_array()
        .ok_or("getAccountInfo: data配列が見つかりません")?;
    let b64_str = data_arr[0]
        .as_str()
        .ok_or("getAccountInfo: base64文字列が見つかりません")?;

    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.decode(b64_str)?;

    parse_resource_limits(&data)
}

/// Borshエンコードされたアカウントデータから ResourceLimits をパースする。
///
/// レイアウト:
///   discriminator(8) + authority(32) + core_mint(32) + ext_mint(32) = 104B固定
///   Vec<[u8;32]> trusted_node_keys (4B len + N×32B)
///   Vec<[u8;32]> trusted_tsa_keys  (4B len + N×32B)
///   Vec<WasmModuleEntry> trusted_wasm_modules (4B len + N×(32+32+4+str_len))
///   ResourceLimitsOnChain (7 × Option<u64>)
fn parse_resource_limits(data: &[u8]) -> Result<ResourceLimits, Box<dyn std::error::Error>> {
    let mut pos: usize = 104; // Skip discriminator + 3 Pubkeys

    // Skip Vec<[u8; 32]> trusted_node_keys
    let node_keys_len = read_u32_le(data, &mut pos)? as usize;
    pos += node_keys_len * 32;

    // Skip Vec<[u8; 32]> trusted_tsa_keys
    let tsa_keys_len = read_u32_le(data, &mut pos)? as usize;
    pos += tsa_keys_len * 32;

    // Skip Vec<WasmModuleEntry> (extension_id[32] + wasm_hash[32] + String)
    let wasm_len = read_u32_le(data, &mut pos)? as usize;
    for _ in 0..wasm_len {
        pos += 32 + 32; // extension_id + wasm_hash
        let str_len = read_u32_le(data, &mut pos)? as usize;
        pos += str_len; // wasm_source string bytes
    }

    // Parse ResourceLimitsOnChain: 7 × Option<u64>
    let max_single_content_bytes = read_option_u64(data, &mut pos)?;
    let max_concurrent_bytes = read_option_u64(data, &mut pos)?;
    let min_upload_speed_bytes = read_option_u64(data, &mut pos)?;
    let base_processing_time_sec = read_option_u64(data, &mut pos)?;
    let max_global_timeout_sec = read_option_u64(data, &mut pos)?;
    let chunk_read_timeout_sec = read_option_u64(data, &mut pos)?;
    let c2pa_max_graph_size = read_option_u64(data, &mut pos)?;

    Ok(ResourceLimits {
        max_single_content_bytes,
        max_concurrent_bytes,
        min_upload_speed_bytes,
        base_processing_time_sec,
        max_global_timeout_sec,
        chunk_read_timeout_sec,
        c2pa_max_graph_size,
    })
}

fn read_u32_le(data: &[u8], pos: &mut usize) -> Result<u32, Box<dyn std::error::Error>> {
    if *pos + 4 > data.len() {
        return Err("データが短すぎます (u32)".into());
    }
    let val = u32::from_le_bytes(data[*pos..*pos + 4].try_into()?);
    *pos += 4;
    Ok(val)
}

fn read_option_u64(data: &[u8], pos: &mut usize) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    if *pos >= data.len() {
        return Err("データが短すぎます (Option tag)".into());
    }
    let tag = data[*pos];
    *pos += 1;
    if tag == 0 {
        Ok(None)
    } else {
        if *pos + 8 > data.len() {
            return Err("データが短すぎます (u64)".into());
        }
        let val = u64::from_le_bytes(data[*pos..*pos + 8].try_into()?);
        *pos += 8;
        Ok(Some(val))
    }
}

/// オンチェーン制限でGatewayのデフォルト値をクランプする。
///
/// 各フィールドについて、オンチェーン値がSomeならmin(gateway, on_chain)を取る。
/// オンチェーン値がNoneならGatewayのデフォルトをそのまま使用する。
pub fn clamp_limits(gateway: &ResourceLimits, on_chain: &ResourceLimits) -> ResourceLimits {
    ResourceLimits {
        max_single_content_bytes: clamp_field(
            gateway.max_single_content_bytes,
            on_chain.max_single_content_bytes,
        ),
        max_concurrent_bytes: clamp_field(
            gateway.max_concurrent_bytes,
            on_chain.max_concurrent_bytes,
        ),
        min_upload_speed_bytes: clamp_field(
            gateway.min_upload_speed_bytes,
            on_chain.min_upload_speed_bytes,
        ),
        base_processing_time_sec: clamp_field(
            gateway.base_processing_time_sec,
            on_chain.base_processing_time_sec,
        ),
        max_global_timeout_sec: clamp_field(
            gateway.max_global_timeout_sec,
            on_chain.max_global_timeout_sec,
        ),
        chunk_read_timeout_sec: clamp_field(
            gateway.chunk_read_timeout_sec,
            on_chain.chunk_read_timeout_sec,
        ),
        c2pa_max_graph_size: clamp_field(
            gateway.c2pa_max_graph_size,
            on_chain.c2pa_max_graph_size,
        ),
    }
}

fn clamp_field(gateway_val: Option<u64>, on_chain_val: Option<u64>) -> Option<u64> {
    match (gateway_val, on_chain_val) {
        (Some(g), Some(o)) => Some(g.min(o)),
        (g, None) => g,
        (None, Some(o)) => Some(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_limits_basic() {
        let gateway = ResourceLimits {
            max_single_content_bytes: Some(2_000_000_000),
            max_concurrent_bytes: Some(8_000_000_000),
            min_upload_speed_bytes: Some(1_000_000),
            base_processing_time_sec: Some(30),
            max_global_timeout_sec: Some(3600),
            chunk_read_timeout_sec: Some(30),
            c2pa_max_graph_size: Some(10000),
        };

        // オンチェーンがより厳しい制限を設定
        let on_chain = ResourceLimits {
            max_single_content_bytes: Some(1_000_000_000), // 1GB < 2GB
            max_concurrent_bytes: None,                     // 制限なし → Gatewayのデフォルト
            min_upload_speed_bytes: Some(2_000_000),        // 2MB/s > 1MB/s → 1MB/s
            base_processing_time_sec: Some(60),             // 60 > 30 → 30
            max_global_timeout_sec: Some(1800),             // 1800 < 3600 → 1800
            chunk_read_timeout_sec: None,
            c2pa_max_graph_size: Some(5000),                // 5000 < 10000 → 5000
        };

        let result = clamp_limits(&gateway, &on_chain);
        assert_eq!(result.max_single_content_bytes, Some(1_000_000_000));
        assert_eq!(result.max_concurrent_bytes, Some(8_000_000_000));
        assert_eq!(result.min_upload_speed_bytes, Some(1_000_000));
        assert_eq!(result.base_processing_time_sec, Some(30));
        assert_eq!(result.max_global_timeout_sec, Some(1800));
        assert_eq!(result.chunk_read_timeout_sec, Some(30));
        assert_eq!(result.c2pa_max_graph_size, Some(5000));
    }

    #[test]
    fn test_clamp_field_both_none() {
        assert_eq!(clamp_field(None, None), None);
    }

    #[test]
    fn test_clamp_field_on_chain_provides_limit() {
        assert_eq!(clamp_field(None, Some(100)), Some(100));
    }

    #[test]
    fn test_parse_resource_limits_minimal() {
        // Build a minimal GlobalConfig account data
        let mut data = Vec::new();

        // discriminator (8 bytes)
        data.extend_from_slice(&[0u8; 8]);
        // authority (32 bytes)
        data.extend_from_slice(&[1u8; 32]);
        // core_collection_mint (32 bytes)
        data.extend_from_slice(&[2u8; 32]);
        // ext_collection_mint (32 bytes)
        data.extend_from_slice(&[3u8; 32]);

        // Vec<[u8;32]> trusted_node_keys — empty
        data.extend_from_slice(&0u32.to_le_bytes());
        // Vec<[u8;32]> trusted_tsa_keys — empty
        data.extend_from_slice(&0u32.to_le_bytes());
        // Vec<WasmModuleEntry> — empty
        data.extend_from_slice(&0u32.to_le_bytes());

        // ResourceLimitsOnChain: 7 × Option<u64>
        // Some(2GB)
        data.push(0x01);
        data.extend_from_slice(&(2u64 * 1024 * 1024 * 1024).to_le_bytes());
        // None
        data.push(0x00);
        // Some(1MB/s)
        data.push(0x01);
        data.extend_from_slice(&(1024u64 * 1024).to_le_bytes());
        // Some(30)
        data.push(0x01);
        data.extend_from_slice(&30u64.to_le_bytes());
        // Some(3600)
        data.push(0x01);
        data.extend_from_slice(&3600u64.to_le_bytes());
        // Some(30)
        data.push(0x01);
        data.extend_from_slice(&30u64.to_le_bytes());
        // Some(10000)
        data.push(0x01);
        data.extend_from_slice(&10000u64.to_le_bytes());

        let limits = parse_resource_limits(&data).unwrap();
        assert_eq!(limits.max_single_content_bytes, Some(2 * 1024 * 1024 * 1024));
        assert_eq!(limits.max_concurrent_bytes, None);
        assert_eq!(limits.min_upload_speed_bytes, Some(1024 * 1024));
        assert_eq!(limits.base_processing_time_sec, Some(30));
        assert_eq!(limits.max_global_timeout_sec, Some(3600));
        assert_eq!(limits.chunk_read_timeout_sec, Some(30));
        assert_eq!(limits.c2pa_max_graph_size, Some(10000));
    }

    #[test]
    fn test_parse_resource_limits_with_vecs() {
        let mut data = Vec::new();

        // Fixed prefix: 104 bytes
        data.extend_from_slice(&[0u8; 8]);   // discriminator
        data.extend_from_slice(&[1u8; 32]);  // authority
        data.extend_from_slice(&[2u8; 32]);  // core_collection_mint
        data.extend_from_slice(&[3u8; 32]);  // ext_collection_mint

        // Vec<[u8;32]> trusted_node_keys — 2 entries
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&[10u8; 32]);
        data.extend_from_slice(&[11u8; 32]);

        // Vec<[u8;32]> trusted_tsa_keys — 1 entry
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[20u8; 32]);

        // Vec<WasmModuleEntry> — 1 entry
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[30u8; 32]); // extension_id
        data.extend_from_slice(&[31u8; 32]); // wasm_hash
        let wasm_source = "ar://test";
        data.extend_from_slice(&(wasm_source.len() as u32).to_le_bytes());
        data.extend_from_slice(wasm_source.as_bytes());

        // ResourceLimitsOnChain: all None
        for _ in 0..7 {
            data.push(0x00);
        }

        let limits = parse_resource_limits(&data).unwrap();
        assert_eq!(limits.max_single_content_bytes, None);
        assert_eq!(limits.max_concurrent_bytes, None);
        assert_eq!(limits.c2pa_max_graph_size, None);
    }
}
