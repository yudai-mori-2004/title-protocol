use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use tokio::sync::{RwLock, Semaphore};

use title_types::{Attribute, SignedJson, SignedJsonCore};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::runtime::mock::MockRuntime;
use crate::runtime::TeeRuntime;
use crate::endpoints::test_helpers::{start_mock_storage, start_inline_proxy};

use super::handler::{handle_sign, b64};

/// テスト用のsigned_jsonを手動構築する
fn build_test_signed_json(rt: &MockRuntime) -> SignedJson {
    let payload = serde_json::json!({
        "content_hash": "0x1234abcdef567890aabbccdd11223344556677889900aabbccddeeff00112233",
        "content_type": "image/jpeg",
        "creator_wallet": "11111111111111111111111111111112",
        "nodes": [{"id": "0x1234abcd", "type": "final"}],
        "links": [],
    });

    let attributes = vec![
        Attribute {
            trait_type: "protocol".to_string(),
            value: "Title-v1".to_string(),
        },
        Attribute {
            trait_type: "content_hash".to_string(),
            value: "0x1234abcdef567890".to_string(),
        },
    ];

    // 署名対象
    let attributes_value = serde_json::to_value(&attributes).unwrap();
    let sign_target = serde_json::json!({
        "payload": payload,
        "attributes": attributes_value,
    });
    let sign_bytes = serde_json::to_vec(&sign_target).unwrap();

    let signature = rt.sign(&sign_bytes);
    let tee_pubkey_b58 = base58::ToBase58::to_base58(rt.signing_pubkey().as_slice());
    let attestation = rt.get_attestation();

    SignedJson {
        core: SignedJsonCore {
            protocol: "Title-v1".to_string(),
            tee_type: "mock".to_string(),
            tee_pubkey: tee_pubkey_b58,
            tee_signature: b64().encode(&signature),
            tee_attestation: b64().encode(&attestation),
        },
        payload,
        attributes,
    }
}

/// signed_jsonを/signに渡し、部分署名済みトランザクションが返ることを確認
#[tokio::test]
async fn test_sign_roundtrip() {
    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();
    rt.generate_tree_keypair();

    // signed_jsonを構築
    let signed_json = build_test_signed_json(&rt);
    let signed_json_bytes = serde_json::to_vec(&signed_json).unwrap();

    // モックストレージとプロキシを起動
    let storage_port = start_mock_storage("/signed_json", signed_json_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // tree_addressを設定（create_tree済みの状態をシミュレート）
    let tree_pubkey_bytes: [u8; 32] = rt.tree_pubkey().try_into().unwrap();

    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        tree_address: RwLock::new(Some(tree_pubkey_bytes)),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
    });

    let body = serde_json::json!({
        "recent_blockhash": "11111111111111111111111111111111",
        "requests": [{
            "signed_json_uri": format!("http://127.0.0.1:{storage_port}/signed_json"),
        }],
    });

    let result = handle_sign(State(state), Json(body)).await;
    assert!(result.is_ok(), "handle_sign failed: {:?}", result.err());

    let response = result.unwrap().0;
    assert_eq!(response.partial_txs.len(), 1);

    // Base64デコードしてトランザクションがデシリアライズ可能
    let tx_bytes = b64().decode(&response.partial_txs[0]).unwrap();
    let tx: solana_sdk::transaction::Transaction = bincode::deserialize(&tx_bytes).unwrap();

    // 2つの署名者（creator_wallet/payer, tee_signing_pubkey）
    assert_eq!(tx.message.header.num_required_signatures, 2);
    // 1つの命令（mint_v2）
    assert_eq!(tx.message.instructions.len(), 1);
}

/// TEE再起動（鍵ローテーション）後に旧signed_jsonが拒否されることを確認
#[tokio::test]
async fn test_sign_rejects_wrong_key() {
    // 旧TEEでsigned_jsonを生成
    let old_rt = MockRuntime::new();
    old_rt.generate_signing_keypair();
    old_rt.generate_encryption_keypair();
    old_rt.generate_tree_keypair();

    let signed_json = build_test_signed_json(&old_rt);
    let signed_json_bytes = serde_json::to_vec(&signed_json).unwrap();

    // 新TEE（鍵がローテーション済み）
    let new_rt = MockRuntime::new();
    new_rt.generate_signing_keypair();
    new_rt.generate_encryption_keypair();
    new_rt.generate_tree_keypair();

    let storage_port = start_mock_storage("/signed_json", signed_json_bytes).await;
    let proxy_port = start_inline_proxy().await;

    let tree_pubkey_bytes: [u8; 32] = new_rt.tree_pubkey().try_into().unwrap();

    let state = Arc::new(TeeAppState {
        runtime: Box::new(new_rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        tree_address: RwLock::new(Some(tree_pubkey_bytes)),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
    });

    let body = serde_json::json!({
        "recent_blockhash": "11111111111111111111111111111111",
        "requests": [{
            "signed_json_uri": format!("http://127.0.0.1:{storage_port}/signed_json"),
        }],
    });

    let result = handle_sign(State(state), Json(body)).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, TeeError::Forbidden(_)));
    assert!(err.to_string().contains("tee_signatureの検証に失敗"));
}

/// サイズ制限を超えるsigned_jsonが拒否されることを確認
#[tokio::test]
async fn test_sign_rejects_oversized() {
    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();
    rt.generate_tree_keypair();

    // 1MBを超えるデータを返すモックストレージ
    let oversized_data = vec![0u8; crate::infra::security::MAX_SIGNED_JSON_SIZE as usize + 1];

    let storage_port = start_mock_storage("/signed_json", oversized_data).await;
    let proxy_port = start_inline_proxy().await;

    let tree_pubkey_bytes: [u8; 32] = rt.tree_pubkey().try_into().unwrap();

    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        tree_address: RwLock::new(Some(tree_pubkey_bytes)),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
    });

    let body = serde_json::json!({
        "recent_blockhash": "11111111111111111111111111111111",
        "requests": [{
            "signed_json_uri": format!("http://127.0.0.1:{storage_port}/signed_json"),
        }],
    });

    let result = handle_sign(State(state), Json(body)).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TeeError::PayloadTooLarge(_)));
}

/// inactive状態での/sign呼び出しが503を返すことを確認
#[tokio::test]
async fn test_sign_inactive_returns_503() {
    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();
    rt.generate_tree_keypair();

    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Inactive),
        proxy_addr: "127.0.0.1:0".to_string(),
        tree_address: RwLock::new(None),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
    });

    let body = serde_json::json!({
        "recent_blockhash": "11111111111111111111111111111111",
        "requests": [],
    });

    let result = handle_sign(State(state), Json(body)).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TeeError::InvalidState(_)));
}
