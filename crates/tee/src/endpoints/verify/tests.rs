use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use title_types::{
    CorePayload, EncryptedPayload, SignedJson, VerifyRequest, VerifyResponse,
};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::runtime::mock::MockRuntime;
use crate::runtime::TeeRuntime;
use crate::endpoints::test_helpers::{start_mock_storage, start_inline_proxy};

use super::{b64, handle_verify};

use std::io::Cursor;
use tokio::sync::{RwLock, Semaphore};

// テストフィクスチャ（core crateから参照）
const CERTS: &[u8] = include_bytes!("../../../../core/tests/fixtures/certs/chain.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../../core/tests/fixtures/certs/ee.key");
const TEST_IMAGE: &[u8] = include_bytes!("../../../../core/tests/fixtures/test.jpg");

/// テスト用signerを作成する（core crateのテストと同一パターン）
fn test_signer() -> Box<dyn c2pa::Signer> {
    c2pa::create_signer::from_keys(CERTS, PRIVATE_KEY, c2pa::SigningAlg::Ed25519, None)
        .unwrap()
}

/// テスト用C2PA署名済みコンテンツを作成する
fn create_signed_content() -> Vec<u8> {
    let manifest_json = serde_json::json!({
        "title": "test-verify.jpg",
        "format": "image/jpeg",
        "claim_generator_info": [{
            "name": "title-tee-test",
            "version": "0.1.0"
        }]
    })
    .to_string();

    let mut builder = c2pa::Builder::from_json(&manifest_json).unwrap();
    let signer = test_signer();

    let mut source = Cursor::new(TEST_IMAGE);
    let mut dest = Cursor::new(Vec::new());
    builder
        .sign(signer.as_ref(), "image/jpeg", &mut source, &mut dest)
        .unwrap();
    dest.into_inner()
}


/// 暗号化ペイロード作成 → /verify → レスポンス復号 → signed_json検証のラウンドトリップテスト
#[tokio::test]
async fn test_verify_roundtrip() {
    // 1. MockRuntime初期化
    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();

    // TEE暗号化公開鍵を取得
    let tee_enc_pubkey_bytes: [u8; 32] = rt.encryption_pubkey().try_into().unwrap();
    let tee_enc_pubkey = X25519PublicKey::from(tee_enc_pubkey_bytes);

    // 2. クライアント側: C2PA署名済みコンテンツを作成
    let signed_content = create_signed_content();
    let content_b64 = b64().encode(&signed_content);

    let client_payload = title_types::ClientPayload {
        owner_wallet: "MockWa11etAddress123456789012345678901234".to_string(),
        content: content_b64,
        sidecar_manifest: None,
        extension_inputs: None,
    };
    let payload_json = serde_json::to_vec(&client_payload).unwrap();

    // 3. クライアント側: ペイロードを暗号化
    let eph_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let eph_pubkey = X25519PublicKey::from(&eph_secret);

    let shared_secret =
        title_crypto::ecdh_derive_shared_secret(&eph_secret, &tee_enc_pubkey);
    let symmetric_key = title_crypto::hkdf_derive_key(&shared_secret).unwrap();

    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let ciphertext =
        title_crypto::aes_gcm_encrypt(&symmetric_key, &nonce, &payload_json).unwrap();

    let encrypted_payload = EncryptedPayload {
        ephemeral_pubkey: b64().encode(eph_pubkey.as_bytes()),
        nonce: b64().encode(nonce),
        ciphertext: b64().encode(&ciphertext),
    };
    let encrypted_payload_bytes = serde_json::to_vec(&encrypted_payload).unwrap();

    // 4. モックTemporary StorageとインラインProxyを起動
    let mock_port = start_mock_storage("/payload", encrypted_payload_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // 5. TeeAppState構築
    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        tree_address: RwLock::new(None),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
    });

    // 6. /verify 呼び出し
    let verify_request = VerifyRequest {
        download_url: format!("http://127.0.0.1:{mock_port}/payload"),
        processor_ids: vec!["core-c2pa".to_string()],
    };
    let body = serde_json::to_value(&verify_request).unwrap();

    let result = handle_verify(State(state.clone()), Json(body)).await;
    assert!(result.is_ok(), "handle_verify failed: {:?}", result.err());

    let encrypted_response = result.unwrap().0;

    // 7. レスポンス復号
    let resp_nonce_bytes = b64().decode(&encrypted_response.nonce).unwrap();
    let resp_nonce: [u8; 12] = resp_nonce_bytes.try_into().unwrap();
    let resp_ct = b64().decode(&encrypted_response.ciphertext).unwrap();

    let resp_plaintext =
        title_crypto::aes_gcm_decrypt(&symmetric_key, &resp_nonce, &resp_ct).unwrap();
    let verify_response: VerifyResponse = serde_json::from_slice(&resp_plaintext).unwrap();

    // 8. signed_json検証
    assert_eq!(verify_response.results.len(), 1);
    let processor_result = &verify_response.results[0];
    assert_eq!(processor_result.processor_id, "core-c2pa");

    let signed_json: SignedJson =
        serde_json::from_value(processor_result.signed_json.clone()).unwrap();
    assert_eq!(signed_json.core.protocol, "Title-v1");
    assert_eq!(signed_json.core.tee_type, "mock");

    // tee_signatureをtee_pubkeyで検証
    use base58::FromBase58;
    let tee_pubkey_bytes = signed_json.core.tee_pubkey.from_base58().unwrap();
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(
        &tee_pubkey_bytes.try_into().expect("公開鍵は32バイト"),
    )
    .expect("有効なEd25519公開鍵");

    let sig_bytes = b64().decode(&signed_json.core.tee_signature).unwrap();
    let signature = ed25519_dalek::Signature::from_bytes(
        &sig_bytes.try_into().expect("署名は64バイト"),
    );

    // 署名対象を再構築して検証
    let sign_target = serde_json::json!({
        "payload": signed_json.payload,
        "attributes": signed_json.attributes,
    });
    let sign_bytes = serde_json::to_vec(&sign_target).unwrap();
    assert!(
        verifying_key.verify_strict(&sign_bytes, &signature).is_ok(),
        "tee_signatureの検証に失敗"
    );

    // content_hashが0xプレフィックス付きhexであることを確認
    let payload: CorePayload =
        serde_json::from_value(signed_json.payload.clone()).unwrap();
    assert!(
        payload.content_hash.starts_with("0x"),
        "content_hashが0xで始まっていません: {}",
        payload.content_hash
    );
    assert_eq!(payload.content_type, "image/jpeg");
    assert_eq!(payload.creator_wallet, "MockWa11etAddress123456789012345678901234");

    // 来歴グラフにルートノードが存在することを確認
    assert!(!payload.nodes.is_empty());
    assert!(payload.nodes.iter().any(|n| n.node_type == "final"));

    // attributesにprotocol, content_hash, content_typeが含まれることを確認
    assert!(signed_json
        .attributes
        .iter()
        .any(|a| a.trait_type == "protocol" && a.value == "Title-v1"));
    assert!(signed_json
        .attributes
        .iter()
        .any(|a| a.trait_type == "content_hash"));
    assert!(signed_json
        .attributes
        .iter()
        .any(|a| a.trait_type == "content_type" && a.value == "image/jpeg"));
}

/// Extension（WASM実行）付き/verifyのテスト
/// processor_ids: ["core-c2pa", "phash-v1"] で両方のsigned_jsonが返ることを確認
#[tokio::test]
async fn test_verify_with_extension() {
    // WASMバイナリをWATから生成（テスト用簡易phash WASM）
    let test_wasm = wat::parse_str(
        r#"(module
        (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
        (import "env" "get_content_length" (func $len (result i32)))
        (import "env" "hash_content" (func $hash (param i32 i32 i32 i32) (result i32)))
        (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
        (memory (export "memory") 1)
        ;; 結果: {"phash":"test"} = 16バイト
        (data (i32.const 1024) "\10\00\00\00{\"phash\":\"test\"}")
        (func (export "alloc") (param i32) (result i32) (i32.const 4096))
        (func (export "process") (result i32)
            (drop (call $len))
            (i32.const 1024)
        )
    )"#,
    )
    .unwrap();

    // テスト用WASMディレクトリを作成
    let wasm_dir = std::env::temp_dir().join("title-test-wasm");
    let _ = std::fs::create_dir_all(&wasm_dir);
    std::fs::write(wasm_dir.join("phash-v1.wasm"), &test_wasm).unwrap();

    // 1. MockRuntime初期化
    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();

    let tee_enc_pubkey_bytes: [u8; 32] = rt.encryption_pubkey().try_into().unwrap();
    let tee_enc_pubkey = X25519PublicKey::from(tee_enc_pubkey_bytes);

    // 2. C2PA署名済みコンテンツ作成・暗号化
    let signed_content = create_signed_content();
    let content_b64 = b64().encode(&signed_content);

    let client_payload = title_types::ClientPayload {
        owner_wallet: "MockWa11etAddress123456789012345678901234".to_string(),
        content: content_b64,
        sidecar_manifest: None,
        extension_inputs: None,
    };
    let payload_json = serde_json::to_vec(&client_payload).unwrap();

    let eph_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let eph_pubkey = X25519PublicKey::from(&eph_secret);
    let shared_secret =
        title_crypto::ecdh_derive_shared_secret(&eph_secret, &tee_enc_pubkey);
    let symmetric_key = title_crypto::hkdf_derive_key(&shared_secret).unwrap();

    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let ciphertext =
        title_crypto::aes_gcm_encrypt(&symmetric_key, &nonce, &payload_json).unwrap();

    let encrypted_payload = EncryptedPayload {
        ephemeral_pubkey: b64().encode(eph_pubkey.as_bytes()),
        nonce: b64().encode(nonce),
        ciphertext: b64().encode(&ciphertext),
    };
    let encrypted_payload_bytes = serde_json::to_vec(&encrypted_payload).unwrap();

    let mock_port = start_mock_storage("/payload", encrypted_payload_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // 3. TeeAppState構築（wasm_dir指定あり）
    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        tree_address: RwLock::new(None),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: Some(Box::new(crate::wasm_loader::FileLoader::new(
            wasm_dir.to_str().unwrap().to_string(),
        ))),
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
    });

    // 4. /verify: core-c2pa + phash-v1
    let verify_request = VerifyRequest {
        download_url: format!("http://127.0.0.1:{mock_port}/payload"),
        processor_ids: vec!["core-c2pa".to_string(), "phash-v1".to_string()],
    };
    let body = serde_json::to_value(&verify_request).unwrap();

    let result = handle_verify(State(state.clone()), Json(body)).await;
    assert!(
        result.is_ok(),
        "handle_verify failed: {:?}",
        result.err()
    );

    let encrypted_response = result.unwrap().0;

    // 5. レスポンス復号
    let resp_nonce_bytes = b64().decode(&encrypted_response.nonce).unwrap();
    let resp_nonce: [u8; 12] = resp_nonce_bytes.try_into().unwrap();
    let resp_ct = b64().decode(&encrypted_response.ciphertext).unwrap();
    let resp_plaintext =
        title_crypto::aes_gcm_decrypt(&symmetric_key, &resp_nonce, &resp_ct).unwrap();
    let verify_response: VerifyResponse =
        serde_json::from_slice(&resp_plaintext).unwrap();

    // 6. 両方のsigned_jsonが返ることを確認
    assert_eq!(
        verify_response.results.len(),
        2,
        "Core + Extension の2結果が返るべき"
    );

    // Core結果
    let core_result = verify_response
        .results
        .iter()
        .find(|r| r.processor_id == "core-c2pa")
        .expect("core-c2pa結果が存在するべき");
    assert_eq!(core_result.signed_json["protocol"], "Title-v1");

    // Extension結果
    let ext_result = verify_response
        .results
        .iter()
        .find(|r| r.processor_id == "phash-v1")
        .expect("phash-v1結果が存在するべき");
    assert_eq!(
        ext_result.signed_json["protocol"],
        "Title-Extension-v1"
    );
    assert_eq!(ext_result.signed_json["extension_id"], "phash-v1");
    // WASM実行結果がpayloadに含まれることを確認
    assert_eq!(
        ext_result.signed_json["payload"]["phash"], "test",
        "WASM実行結果のphashがpayloadに含まれるべき"
    );

    // クリーンアップ
    let _ = std::fs::remove_dir_all(&wasm_dir);
}

/// inactive状態での/verify呼び出しが503を返すことを確認
#[tokio::test]
async fn test_verify_inactive_returns_503() {
    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();

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
        "download_url": "http://example.com/payload",
        "processor_ids": ["core-c2pa"],
    });

    let result = handle_verify(State(state), Json(body)).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TeeError::InvalidState(_)));
}

/// 信頼されていないextension_idのWASM実行が拒否されることを確認
/// 仕様書 §6.4 不正WASMインジェクション防御
#[tokio::test]
async fn test_verify_rejects_untrusted_extension() {
    // WASMバイナリを用意
    let test_wasm = wat::parse_str(
        r#"(module
        (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
        (import "env" "get_content_length" (func $len (result i32)))
        (import "env" "hash_content" (func $hash (param i32 i32 i32 i32) (result i32)))
        (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 1024) "\10\00\00\00{\"phash\":\"test\"}")
        (func (export "alloc") (param i32) (result i32) (i32.const 4096))
        (func (export "process") (result i32)
            (drop (call $len))
            (i32.const 1024)
        )
    )"#,
    )
    .unwrap();

    let wasm_dir = std::env::temp_dir().join("title-test-wasm-untrusted");
    let _ = std::fs::create_dir_all(&wasm_dir);
    std::fs::write(wasm_dir.join("evil-ext.wasm"), &test_wasm).unwrap();

    let rt = MockRuntime::new();
    rt.generate_signing_keypair();
    rt.generate_encryption_keypair();

    let tee_enc_pubkey_bytes: [u8; 32] = rt.encryption_pubkey().try_into().unwrap();
    let tee_enc_pubkey = X25519PublicKey::from(tee_enc_pubkey_bytes);

    let signed_content = create_signed_content();
    let content_b64 = b64().encode(&signed_content);

    let client_payload = title_types::ClientPayload {
        owner_wallet: "MockWa11etAddress123456789012345678901234".to_string(),
        content: content_b64,
        sidecar_manifest: None,
        extension_inputs: None,
    };
    let payload_json = serde_json::to_vec(&client_payload).unwrap();

    let eph_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let eph_pubkey = X25519PublicKey::from(&eph_secret);
    let shared_secret =
        title_crypto::ecdh_derive_shared_secret(&eph_secret, &tee_enc_pubkey);
    let symmetric_key = title_crypto::hkdf_derive_key(&shared_secret).unwrap();

    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let ciphertext =
        title_crypto::aes_gcm_encrypt(&symmetric_key, &nonce, &payload_json).unwrap();

    let encrypted_payload = EncryptedPayload {
        ephemeral_pubkey: b64().encode(eph_pubkey.as_bytes()),
        nonce: b64().encode(nonce),
        ciphertext: b64().encode(&ciphertext),
    };
    let encrypted_payload_bytes = serde_json::to_vec(&encrypted_payload).unwrap();

    let mock_port = start_mock_storage("/payload", encrypted_payload_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // trusted_extension_idsに "phash-v1" のみ許可（"evil-ext" は不許可）
    let mut trusted = std::collections::HashSet::new();
    trusted.insert("phash-v1".to_string());

    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        tree_address: RwLock::new(None),
        collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: Some(Box::new(crate::wasm_loader::FileLoader::new(
            wasm_dir.to_str().unwrap().to_string(),
        ))),
        memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
        trusted_extension_ids: Some(trusted),
    });

    // "evil-ext" を含む /verify リクエスト → 拒否されるべき
    let verify_request = VerifyRequest {
        download_url: format!("http://127.0.0.1:{mock_port}/payload"),
        processor_ids: vec!["core-c2pa".to_string(), "evil-ext".to_string()],
    };
    let body = serde_json::to_value(&verify_request).unwrap();

    let result = handle_verify(State(state), Json(body)).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(&err, TeeError::Forbidden(_)));
    let msg = format!("{err}");
    assert!(
        msg.contains("信頼されていないExtension ID"),
        "エラーメッセージに '信頼されていないExtension ID' が含まれるべき: {msg}"
    );

    let _ = std::fs::remove_dir_all(&wasm_dir);
}
