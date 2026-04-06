// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use title_types::{
    CorePayload, SignedJson, VerifyRequest, VerifyResponse,
};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::runtime::mock::MockRuntime;
use crate::runtime::TeeRuntime;
use crate::endpoints::test_helpers::{start_mock_storage, start_inline_proxy};

use super::handle_verify;

use std::io::Cursor;
use tokio::sync::RwLock;

/// バイナリ暗号化ペイロードを構築するテストヘルパー。
///
/// 平文: [4B: metadata_len][metadata JSON][raw content]
/// seal_for() でKEM + KDF + AEADを一括処理し、ワイヤーフォーマットを構築。
/// ResponseSealerを返すので、テスト側でレスポンス復号に使用する。
fn build_binary_payload(
    content: &[u8],
    owner_wallet: &str,
    tee_enc_pubkey_bytes: &[u8; 32],
) -> (Vec<u8>, title_crypto::ResponseSealer) {
    // メタデータJSON
    let metadata = title_types::ClientMetadata {
        owner_wallet: owner_wallet.to_string(),
        extension_inputs: None,
    };
    let metadata_json = serde_json::to_vec(&metadata).unwrap();

    // 平文: [4B metadata_len][metadata JSON][raw content]
    let mut plaintext = Vec::new();
    plaintext.extend_from_slice(&(metadata_json.len() as u32).to_be_bytes());
    plaintext.extend_from_slice(&metadata_json);
    plaintext.extend_from_slice(content);

    // Encapsulatorを構築してseal_forで暗号化
    let encapsulator = title_crypto::impls::x25519::X25519Encapsulator::from_public_key(*tee_enc_pubkey_bytes);
    let (wire, response_sealer) = title_crypto::seal_for(
        &encapsulator,
        &plaintext,
        b"/verify",
    ).unwrap();

    (wire, response_sealer)
}

// テストフィクスチャ（共有テストフィクスチャディレクトリ）
const CERTS: &[u8] = include_bytes!("../../../../../tests/fixtures/certs/chain.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../../../tests/fixtures/certs/ee.key");
const TEST_IMAGE: &[u8] = include_bytes!("../../../../../tests/fixtures/minimal/test.jpg");

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
    rt.generate_keypairs();

    // TEE暗号化公開鍵を取得
    let tee_enc_pubkey_bytes: [u8; 32] = rt.decapsulator().public_key_bytes().try_into().unwrap();

    // 2. クライアント側: C2PA署名済みコンテンツを作成・暗号化
    let signed_content = create_signed_content();
    let (payload_bytes, response_sealer) = build_binary_payload(
        &signed_content,
        "MockWa11etAddress123456789012345678901234",
        &tee_enc_pubkey_bytes,
    );

    // 3. モックTemporary StorageとインラインProxyを起動
    let mock_port = start_mock_storage("/payload", payload_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // 5. TeeAppState構築
    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        core_tree_address: RwLock::new(None),
        ext_tree_address: RwLock::new(None),
        core_collection_mint: None,
        ext_collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        resource_pool: Arc::new(title_wasm_host::ResourcePool::with_single_limit(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
        alt_address: RwLock::new(None),
        alt_addresses: RwLock::new(vec![]),
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

    // 7. レスポンス復号（ResponseSealer.open()を使用）
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let resp_nonce_bytes = b64.decode(&encrypted_response.nonce).unwrap();
    let resp_ct = b64.decode(&encrypted_response.ciphertext).unwrap();

    // nonce || ciphertext を結合してResponseSealer.open()で復号
    let mut nonce_and_ct = Vec::with_capacity(resp_nonce_bytes.len() + resp_ct.len());
    nonce_and_ct.extend_from_slice(&resp_nonce_bytes);
    nonce_and_ct.extend_from_slice(&resp_ct);
    let resp_plaintext = response_sealer.open(&nonce_and_ct, b"/verify").unwrap();
    let verify_response: VerifyResponse = serde_json::from_slice(&resp_plaintext).unwrap();

    // 8. signed_json検証
    assert_eq!(verify_response.results.len(), 1);
    let processor_result = &verify_response.results[0];
    assert_eq!(processor_result.processor_id, "core-c2pa");

    let signed_json: SignedJson =
        serde_json::from_value(processor_result.signed_json.clone()).unwrap();
    assert_eq!(signed_json.core.protocol, "Title-v1");
    assert_eq!(signed_json.core.tee_type, "mock");

    // tee_signatureをtee_pubkeyで検証（ドメインタグ付き）
    let tee_pubkey_bytes = b64.decode(&signed_json.core.tee_pubkey).unwrap();
    let verifier = title_crypto::create_verifier(
        title_crypto::SigningAlgorithm::Ed25519,
        &tee_pubkey_bytes,
    ).unwrap();

    let sig_bytes = b64.decode(&signed_json.core.tee_signature).unwrap();

    // 署名対象を再構築して検証
    let sign_target = serde_json::json!({
        "payload": signed_json.payload,
        "attributes": signed_json.attributes,
    });
    let sign_bytes = serde_json_canonicalizer::to_vec(&sign_target).unwrap();
    let tagged = title_crypto::domain_tagged("title-protocol-v1", &sign_bytes);
    assert!(
        verifier.verify(&tagged, &sig_bytes).is_ok(),
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
/// processor_ids: ["core-c2pa", "image-phash"] で両方のsigned_jsonが返ることを確認
#[tokio::test]
async fn test_verify_with_extension() {
    // WASMバイナリをWATから生成（テスト用簡易phash WASM）
    let test_wasm = wat::parse_str(
        r#"(module
        (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
        (import "env" "get_content_length" (func $len (result i32)))
        (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
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
    std::fs::write(wasm_dir.join("image-phash.wasm"), &test_wasm).unwrap();

    // 1. MockRuntime初期化
    let rt = MockRuntime::new();
    rt.generate_keypairs();

    let tee_enc_pubkey_bytes: [u8; 32] = rt.decapsulator().public_key_bytes().try_into().unwrap();

    // 2. C2PA署名済みコンテンツ作成・暗号化
    let signed_content = create_signed_content();
    let (payload_bytes, response_sealer) = build_binary_payload(
        &signed_content,
        "MockWa11etAddress123456789012345678901234",
        &tee_enc_pubkey_bytes,
    );

    let mock_port = start_mock_storage("/payload", payload_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // 3. TeeAppState構築（wasm_dir指定あり）
    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        core_tree_address: RwLock::new(None),
        ext_tree_address: RwLock::new(None),
        core_collection_mint: None,
        ext_collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: Some(Box::new(crate::wasm_loader::FileLoader::new(
            wasm_dir.to_str().unwrap().to_string(),
        ))),
        resource_pool: Arc::new(title_wasm_host::ResourcePool::with_single_limit(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
        alt_address: RwLock::new(None),
        alt_addresses: RwLock::new(vec![]),
    });

    // 4. /verify: core-c2pa + phash-v1
    let verify_request = VerifyRequest {
        download_url: format!("http://127.0.0.1:{mock_port}/payload"),
        processor_ids: vec!["core-c2pa".to_string(), "image-phash".to_string()],
    };
    let body = serde_json::to_value(&verify_request).unwrap();

    let result = handle_verify(State(state.clone()), Json(body)).await;
    assert!(
        result.is_ok(),
        "handle_verify failed: {:?}",
        result.err()
    );

    let encrypted_response = result.unwrap().0;

    // 5. レスポンス復号（ResponseSealer.open()を使用）
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let resp_nonce_bytes = b64.decode(&encrypted_response.nonce).unwrap();
    let resp_ct = b64.decode(&encrypted_response.ciphertext).unwrap();
    let mut nonce_and_ct = Vec::with_capacity(resp_nonce_bytes.len() + resp_ct.len());
    nonce_and_ct.extend_from_slice(&resp_nonce_bytes);
    nonce_and_ct.extend_from_slice(&resp_ct);
    let resp_plaintext = response_sealer.open(&nonce_and_ct, b"/verify").unwrap();
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
        .find(|r| r.processor_id == "image-phash")
        .expect("phash-v1結果が存在するべき");
    assert_eq!(
        ext_result.signed_json["protocol"],
        "Title-Extension-v1"
    );
    // Extension signed_jsonもSignedJson構造体を使用するため、
    // extension_idはpayload内にある
    assert_eq!(ext_result.signed_json["payload"]["extension_id"], "image-phash");
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
    rt.generate_keypairs();

    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Inactive),
        proxy_addr: "127.0.0.1:0".to_string(),
        core_tree_address: RwLock::new(None),
        ext_tree_address: RwLock::new(None),
        core_collection_mint: None,
        ext_collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: None,
        resource_pool: Arc::new(title_wasm_host::ResourcePool::with_single_limit(1024 * 1024 * 1024)),
        trusted_extension_ids: None,
        alt_address: RwLock::new(None),
        alt_addresses: RwLock::new(vec![]),
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
        (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
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
    rt.generate_keypairs();

    let tee_enc_pubkey_bytes: [u8; 32] = rt.decapsulator().public_key_bytes().try_into().unwrap();

    let signed_content = create_signed_content();
    let (payload_bytes, _response_sealer) = build_binary_payload(
        &signed_content,
        "MockWa11etAddress123456789012345678901234",
        &tee_enc_pubkey_bytes,
    );

    let mock_port = start_mock_storage("/payload", payload_bytes).await;
    let proxy_port = start_inline_proxy().await;

    // trusted_extension_idsに "image-phash" のみ許可（"evil-ext" は不許可）
    let mut trusted = std::collections::HashSet::new();
    trusted.insert("image-phash".to_string());

    let state = Arc::new(TeeAppState {
        runtime: Box::new(rt),
        state: RwLock::new(TeeState::Active),
        proxy_addr: format!("127.0.0.1:{proxy_port}"),
        core_tree_address: RwLock::new(None),
        ext_tree_address: RwLock::new(None),
        core_collection_mint: None,
        ext_collection_mint: None,
        gateway_pubkey: None,
        wasm_loader: Some(Box::new(crate::wasm_loader::FileLoader::new(
            wasm_dir.to_str().unwrap().to_string(),
        ))),
        resource_pool: Arc::new(title_wasm_host::ResourcePool::with_single_limit(1024 * 1024 * 1024)),
        trusted_extension_ids: Some(trusted),
        alt_address: RwLock::new(None),
        alt_addresses: RwLock::new(vec![]),
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

// ---------------------------------------------------------------------------
// ユーティリティ関数テスト
// ---------------------------------------------------------------------------

/// JPEGマジックバイト検出
#[test]
fn test_detect_mime_type_jpeg() {
    let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00];
    assert_eq!(super::detect_mime_type(&data), "image/jpeg");
}

/// PNGマジックバイト検出
#[test]
fn test_detect_mime_type_png() {
    let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A];
    assert_eq!(super::detect_mime_type(&data), "image/png");
}

/// WEBPマジックバイト検出
#[test]
fn test_detect_mime_type_webp() {
    let mut data = [0u8; 16];
    data[8..12].copy_from_slice(b"WEBP");
    assert_eq!(super::detect_mime_type(&data), "image/webp");
}

/// 未知のフォーマットはapplication/octet-streamにフォールバック
#[test]
fn test_detect_mime_type_unknown() {
    assert_eq!(super::detect_mime_type(b"unknown"), "application/octet-stream");
    assert_eq!(super::detect_mime_type(&[]), "application/octet-stream");
}

/// content_hashフォーマット: 0xプレフィックス + 64文字hex
#[test]
fn test_format_content_hash() {
    let hash = [0u8; 32];
    let result = super::format_content_hash(&hash);
    assert_eq!(result, "0x0000000000000000000000000000000000000000000000000000000000000000");

    let hash2 = [0xFF; 32];
    let result2 = super::format_content_hash(&hash2);
    assert_eq!(result2, "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
}
