// SPDX-License-Identifier: Apache-2.0

//! # /register-node エンドポイント
//!
//! 仕様書 §8.2
//!
//! TEEノードのオンチェーン登録トランザクションを構築・署名する。
//! `/create-tree` と同様、TEE内部の署名鍵でトランザクションに署名し、
//! スペック（encryption_pubkey, gateway_endpoint等）の真正性を暗号的に保証する。
//!
//! ## フロー
//! 1. ノード運営者が `/register-node` を呼び出す
//! 2. TEEが `register_tee_node` Anchor命令のトランザクションを構築
//! 3. TEEが自身の signing_key で部分署名（payer + 鍵所有証明）
//! 4. 部分署名済みトランザクションを返却
//! 5. ノード運営者がDAOの authority に共同署名を依頼
//! 6. 完全署名されたトランザクションをブロードキャスト

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use sha2::{Digest, Sha256};
#[allow(deprecated)] // solana-sdk 2.x のsystem_program非推奨警告を抑制
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::Signature,
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;

use title_types::{RegisterNodeRequest, RegisterNodeResponse};

use crate::blockchain::solana_tx;
use crate::config::TeeAppState;
use crate::error::TeeError;

use super::b64;

/// Anchor instruction discriminator: sha256("global:register_tee_node")[..8]
fn anchor_disc_register_tee_node() -> [u8; 8] {
    let hash = Sha256::digest(b"global:register_tee_node");
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

/// GlobalConfig PDA導出。seeds = [b"global-config"]
fn find_global_config_pda(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"global-config"], program_id)
}

/// TeeNodeAccount PDA導出。seeds = [b"tee-node", &signing_pubkey]
fn find_tee_node_pda(signing_pubkey: &[u8; 32], program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"tee-node", signing_pubkey.as_ref()], program_id)
}

/// Borsh String encode: 4-byte LE length + UTF-8 bytes
fn borsh_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut buf = Vec::with_capacity(4 + bytes.len());
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    buf
}

/// /register-node エンドポイントハンドラ。
/// 仕様書 §8.2
///
/// TEE内部で `register_tee_node` Anchor命令を構築し、
/// TEEの署名鍵で部分署名して返す。
/// authorityの共同署名後にブロードキャスト可能となる。
pub async fn handle_register_node(
    State(state): State<Arc<TeeAppState>>,
    Json(request): Json<RegisterNodeRequest>,
) -> Result<Json<RegisterNodeResponse>, TeeError> {
    // プログラムID
    let program_id = Pubkey::from_str(&request.program_id)
        .map_err(|e| TeeError::BadRequest(format!("program_idのパースに失敗: {e}")))?;

    // Authority
    let authority = Pubkey::from_str(&request.authority)
        .map_err(|e| TeeError::BadRequest(format!("authorityのパースに失敗: {e}")))?;

    // Gateway pubkey
    let gateway_pubkey_bytes: [u8; 32] = {
        let pk = Pubkey::from_str(&request.gateway_pubkey)
            .map_err(|e| TeeError::BadRequest(format!("gateway_pubkeyのパースに失敗: {e}")))?;
        pk.to_bytes()
    };

    // Blockhash
    let blockhash = solana_sdk::hash::Hash::from_str(&request.recent_blockhash)
        .map_err(|e| TeeError::BadRequest(format!("recent_blockhashのパースに失敗: {e}")))?;

    // TEE鍵取得
    let signing_pubkey_bytes: [u8; 32] = state
        .runtime
        .signing_pubkey()
        .try_into()
        .map_err(|_| TeeError::Internal("署名用公開鍵の取得に失敗".into()))?;
    let tee_signing_pubkey = Pubkey::new_from_array(signing_pubkey_bytes);

    let encryption_pubkey_bytes: Vec<u8> = state.runtime.encryption_pubkey();
    let encryption_pubkey_arr: [u8; 32] = encryption_pubkey_bytes
        .clone()
        .try_into()
        .map_err(|_| TeeError::Internal("暗号化用公開鍵の取得に失敗".into()))?;

    // tee_type: runtime名からu8に変換
    let tee_type: u8 = match state.runtime.tee_type() {
        "aws_nitro" | "nitro" => 0,
        "amd_sev_snp" => 1,
        "intel_tdx" => 2,
        _ => 0, // mock等はaws_nitroとして扱う
    };

    // コレクションMint
    let core_collection_mint = Pubkey::from_str(&request.core_collection_mint)
        .map_err(|e| TeeError::BadRequest(format!("core_collection_mintのパースに失敗: {e}")))?;
    let ext_collection_mint = Pubkey::from_str(&request.ext_collection_mint)
        .map_err(|e| TeeError::BadRequest(format!("ext_collection_mintのパースに失敗: {e}")))?;

    // MPL Core プログラムID
    let mpl_core_program = Pubkey::from_str("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap();

    // PDA導出
    let (global_config_pda, _) = find_global_config_pda(&program_id);
    let (tee_node_pda, _) = find_tee_node_pda(&signing_pubkey_bytes, &program_id);

    // MeasurementEntry構築: key=[u8;16] (null-padded), value=[u8;48]
    // 仕様書 §5.2 Step 4 — キー名と値の解釈は tee_type に依存する
    let measurement_entries: Vec<([u8; 16], [u8; 48])> = {
        let mut entries = Vec::new();
        for (key, hex_val) in &request.measurements {
            let mut key_buf = [0u8; 16];
            let key_bytes = key.as_bytes();
            let copy_len = key_bytes.len().min(16);
            key_buf[..copy_len].copy_from_slice(&key_bytes[..copy_len]);

            let val_bytes = hex::decode(hex_val).map_err(|e| {
                TeeError::BadRequest(format!("measurement値のhexデコードに失敗 ({key}): {e}"))
            })?;
            let mut val_buf = [0u8; 48];
            let val_len = val_bytes.len().min(48);
            val_buf[..val_len].copy_from_slice(&val_bytes[..val_len]);

            entries.push((key_buf, val_buf));
        }
        entries
    };

    // Anchor命令データ構築
    // register_tee_node(signing_pubkey, encryption_pubkey, gateway_pubkey,
    //                   gateway_endpoint, tee_type, measurements)
    let mut ix_data = Vec::new();
    ix_data.extend_from_slice(&anchor_disc_register_tee_node());
    ix_data.extend_from_slice(&signing_pubkey_bytes); // signing_pubkey: [u8; 32]
    ix_data.extend_from_slice(&encryption_pubkey_arr); // encryption_pubkey: [u8; 32]
    ix_data.extend_from_slice(&gateway_pubkey_bytes); // gateway_pubkey: [u8; 32]
    ix_data.extend_from_slice(&borsh_string(&request.gateway_endpoint)); // gateway_endpoint: String
    ix_data.push(tee_type); // tee_type: u8
    // measurements: Vec<MeasurementEntry>
    ix_data.extend_from_slice(&(measurement_entries.len() as u32).to_le_bytes());
    for (key, value) in &measurement_entries {
        ix_data.extend_from_slice(key);   // [u8; 16]
        ix_data.extend_from_slice(value);  // [u8; 48]
    }

    // Anchor accounts (RegisterTeeNode構造体の順序に合わせる)
    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(global_config_pda, false),         // global_config (mut)
            AccountMeta::new(tee_node_pda, false),              // tee_node (mut, init)
            AccountMeta::new(authority, true),                   // authority (mut, signer) — MPL Core CPI payer
            AccountMeta::new(tee_signing_pubkey, true),          // payer/TEE (mut, signer)
            AccountMeta::new(core_collection_mint, false),       // core_collection (mut)
            AccountMeta::new(ext_collection_mint, false),        // ext_collection (mut)
            AccountMeta::new_readonly(mpl_core_program, false),  // mpl_core_program
            AccountMeta::new_readonly(system_program::id(), false), // system_program
        ],
        data: ix_data,
    };

    // トランザクション構築（TEEがfee payer）
    let message = Message::new_with_blockhash(&[ix], Some(&tee_signing_pubkey), &blockhash);

    let num_signers = message.header.num_required_signatures as usize;
    let signatures = vec![Signature::default(); num_signers];
    let mut tx = Transaction {
        signatures,
        message,
    };

    // TEE署名（payer署名）
    let message_bytes = tx.message.serialize();
    let signing_sig = state.runtime.sign(&message_bytes);
    solana_tx::apply_partial_signature(&mut tx, &tee_signing_pubkey, &signing_sig)
        .map_err(|e| TeeError::Internal(format!("TEE署名の適用に失敗: {e}")))?;

    // シリアライズ
    let tx_bytes = solana_tx::serialize_transaction(&tx)
        .map_err(|e| TeeError::Internal(format!("トランザクションのシリアライズに失敗: {e}")))?;

    tracing::info!(
        tee_node_pda = %tee_node_pda,
        signing_pubkey = %tee_signing_pubkey,
        "TEEノード登録トランザクションを構築しました（authority共同署名待ち）"
    );

    Ok(Json(RegisterNodeResponse {
        partial_tx: b64().encode(&tx_bytes),
        signing_pubkey: tee_signing_pubkey.to_string(),
        encryption_pubkey: b64().encode(&encryption_pubkey_bytes),
        tee_node_pda: tee_node_pda.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TeeState;
    use crate::runtime::mock::MockRuntime;
    use crate::runtime::TeeRuntime;
    use tokio::sync::{RwLock, Semaphore};

    fn make_test_state() -> Arc<TeeAppState> {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();
        rt.generate_tree_keypair();
        rt.generate_ext_tree_keypair();

        Arc::new(TeeAppState {
            runtime: Box::new(rt),
            state: RwLock::new(TeeState::Inactive),
            proxy_addr: "127.0.0.1:0".to_string(),
            core_tree_address: RwLock::new(None),
            ext_tree_address: RwLock::new(None),
            core_collection_mint: None,
            ext_collection_mint: None,
            gateway_pubkey: None,
            wasm_loader: None,
            memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
            trusted_extension_ids: None,
            wasm_memory_pool: Arc::new(title_wasm_host::MemoryPool::new(1024 * 1024 * 1024)),
        })
    }

    /// /register-node が部分署名済みトランザクションを返すことを確認
    #[tokio::test]
    async fn test_register_node_success() {
        let state = make_test_state();

        let request = RegisterNodeRequest {
            gateway_endpoint: "http://localhost:3000".to_string(),
            gateway_pubkey: Pubkey::new_unique().to_string(),
            recent_blockhash: "11111111111111111111111111111111".to_string(),
            authority: Pubkey::new_unique().to_string(),
            program_id: "5p5Tf93fEbCPZxA1NG48rH9ozDALsVmVVf52QW3VDNoN".to_string(),
            core_collection_mint: Pubkey::new_unique().to_string(),
            ext_collection_mint: Pubkey::new_unique().to_string(),
            measurements: Default::default(),
        };

        let result = handle_register_node(State(state.clone()), Json(request)).await;
        assert!(result.is_ok(), "handle_register_node failed: {:?}", result.err());

        let response = result.unwrap().0;

        // partial_txがBase64デコード可能
        let tx_bytes = b64().decode(&response.partial_tx).unwrap();
        assert!(!tx_bytes.is_empty());

        // トランザクションがデシリアライズ可能
        let tx: Transaction = bincode::deserialize(&tx_bytes).unwrap();
        // 2署名者: TEE(payer) + authority
        assert_eq!(tx.message.header.num_required_signatures, 2);
        // 1命令: register_tee_node
        assert_eq!(tx.message.instructions.len(), 1);

        // signing_pubkeyがBase58
        assert!(!response.signing_pubkey.is_empty());
        Pubkey::from_str(&response.signing_pubkey).unwrap();

        // encryption_pubkeyがBase64で32バイト
        let enc_pk = b64().decode(&response.encryption_pubkey).unwrap();
        assert_eq!(enc_pk.len(), 32);

        // tee_node_pdaがBase58
        Pubkey::from_str(&response.tee_node_pda).unwrap();
    }

    /// payer(TEE)の公開鍵がトランザクションのfee payerであることを確認
    #[tokio::test]
    async fn test_register_node_tee_is_payer() {
        let state = make_test_state();

        let request = RegisterNodeRequest {
            gateway_endpoint: "http://example.com:3000".to_string(),
            gateway_pubkey: Pubkey::new_unique().to_string(),
            recent_blockhash: "11111111111111111111111111111111".to_string(),
            authority: Pubkey::new_unique().to_string(),
            program_id: "5p5Tf93fEbCPZxA1NG48rH9ozDALsVmVVf52QW3VDNoN".to_string(),
            core_collection_mint: Pubkey::new_unique().to_string(),
            ext_collection_mint: Pubkey::new_unique().to_string(),
            measurements: Default::default(),
        };

        let result = handle_register_node(State(state.clone()), Json(request)).await.unwrap();
        let tx_bytes = b64().decode(&result.0.partial_tx).unwrap();
        let tx: Transaction = bincode::deserialize(&tx_bytes).unwrap();

        // fee payer (account_keys[0]) == TEE signing_pubkey
        let tee_pubkey = Pubkey::from_str(&result.0.signing_pubkey).unwrap();
        assert_eq!(tx.message.account_keys[0], tee_pubkey);

        // TEEの署名スロットが埋まっている（default署名ではない）
        assert_ne!(tx.signatures[0], Signature::default());
    }
}
