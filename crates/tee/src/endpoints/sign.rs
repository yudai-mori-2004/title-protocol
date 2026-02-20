//! # /sign エンドポイント
//!
//! 仕様書 §6.4 /signフェーズの内部処理
//!
//! ## 処理フロー
//! 1. signed_json_uriからJSONをフェッチ（サイズ制限: 1MB）
//! 2. JSON内のtee_signatureを自身の公開鍵で検証
//! 3. payload.creator_walletを宛先としてBubblegum V2 cNFT発行トランザクションを構築
//! 4. TEEの秘密鍵で部分署名
//!
//! ## 防御策（Verify on Sign）
//! - JSONフェッチ時のサイズ制限（1MB上限）
//! - tee_signature検証によるTEE再起動時の自動拒否

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use ed25519_dalek::VerifyingKey;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use title_types::{SignRequest, SignResponse, SignedJson};

use crate::security::{self, SecurityError};
use crate::solana_tx;
use crate::{AppState, TeeState};

/// Base64エンジン（Standard）
fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// /sign エンドポイントハンドラ。
/// 仕様書 §6.4
pub async fn handle_sign(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<SignResponse>, (axum::http::StatusCode, String)> {
    // active状態チェック
    {
        let current = state.state.read().await;
        if *current != TeeState::Active {
            return Err((
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "TEEはまだactive状態ではありません".to_string(),
            ));
        }
    }

    // Step 1. Gateway署名の検証（§6.2）
    let (inner_body, resource_limits) =
        crate::gateway_auth::verify_gateway_auth(state.gateway_pubkey.as_ref(), &body)?;

    let request: SignRequest = serde_json::from_value(inner_body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("SignRequestのパースに失敗: {e}"),
        )
    })?;

    // resource_limitsの適用（§6.4）
    let limits = security::resolve_limits(resource_limits.as_ref());
    let chunk_timeout = Duration::from_secs(limits.chunk_read_timeout_sec);

    // recent_blockhash（Base58デコード）
    let blockhash =
        solana_sdk::hash::Hash::from_str(&request.recent_blockhash).map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("recent_blockhashのBase58デコードに失敗: {e}"),
            )
        })?;

    // TEE署名用公開鍵
    let tee_pubkey_bytes: [u8; 32] = state.runtime.signing_pubkey().try_into().map_err(|_| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "署名用公開鍵の取得に失敗".to_string(),
        )
    })?;
    let tee_signing_pubkey = Pubkey::new_from_array(tee_pubkey_bytes);

    // Ed25519検証用キー
    let verifying_key = VerifyingKey::from_bytes(&tee_pubkey_bytes).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("検証用公開鍵の構築に失敗: {e}"),
        )
    })?;

    // Tree address
    let tree_address_bytes = {
        let tree_addr = state.tree_address.read().await;
        tree_addr.ok_or((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Merkle Treeが未作成です。先に/create-treeを呼び出してください".to_string(),
        ))?
    };
    let tree_pubkey = Pubkey::new_from_array(tree_address_bytes);

    // コレクションアドレス
    let collection_mint = state.collection_mint.as_ref();

    let mut partial_txs = Vec::new();

    for item in &request.requests {
        // Step 1: signed_json_uriからJSONをフェッチ（セキュア化: サイズ制限+チャンクタイムアウト+セマフォ）
        // 仕様書 §6.4 /signフェーズでの防御（Verify on Sign）
        let proxy_response = security::proxy_get_secured(
            &state.proxy_addr,
            &item.signed_json_uri,
            security::MAX_SIGNED_JSON_SIZE,
            chunk_timeout,
            &state.memory_semaphore,
        )
        .await
        .map_err(|e| match &e {
            SecurityError::PayloadTooLarge { .. } => (
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                format!("signed_jsonのサイズが上限を超えています: {e}"),
            ),
            SecurityError::MemoryLimitExceeded => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                e.to_string(),
            ),
            SecurityError::ChunkReadTimeout { .. } => (
                axum::http::StatusCode::REQUEST_TIMEOUT,
                e.to_string(),
            ),
            SecurityError::ProxyError(status) => (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("オフチェーンストレージがエラーを返しました: HTTP {status}"),
            ),
            _ => (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("signed_jsonの取得に失敗: {e}"),
            ),
        })?;

        // signed_jsonをパース
        let signed_json: SignedJson =
            serde_json::from_slice(&proxy_response.body).map_err(|e| {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    format!("signed_jsonのパースに失敗: {e}"),
                )
            })?;

        // Step 2: tee_signatureを自身の公開鍵で検証
        // 仕様書 §6.4: 自身が生成したsigned_jsonであることの確認
        // TEE再起動（鍵ローテーション）後は旧signed_jsonが自動的に拒否される
        let sig_bytes = b64().decode(&signed_json.core.tee_signature).map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("tee_signatureのBase64デコードに失敗: {e}"),
            )
        })?;
        let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                "tee_signatureは64バイトである必要があります".to_string(),
            )
        })?;
        let ed_signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

        // 署名対象を再構築して検証
        let sign_target = serde_json::json!({
            "payload": signed_json.payload,
            "attributes": signed_json.attributes,
        });
        let sign_bytes = serde_json::to_vec(&sign_target).map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("署名対象のシリアライズに失敗: {e}"),
            )
        })?;

        verifying_key
            .verify_strict(&sign_bytes, &ed_signature)
            .map_err(|_| {
                (
                    axum::http::StatusCode::FORBIDDEN,
                    "tee_signatureの検証に失敗しました。TEEが再起動した可能性があります"
                        .to_string(),
                )
            })?;

        // Step 3: Bubblegum V2 cNFT発行トランザクション構築
        // creator_walletを取得（仕様書 §5.1 Step 9）
        let creator_wallet_str = signed_json
            .payload
            .get("creator_wallet")
            .and_then(|v| v.as_str())
            .ok_or((
                axum::http::StatusCode::BAD_REQUEST,
                "signed_json.payload.creator_walletが見つかりません".to_string(),
            ))?;
        let creator_wallet = Pubkey::from_str(creator_wallet_str).map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("creator_walletのBase58デコードに失敗: {e}"),
            )
        })?;

        // content_hashを取得
        let content_hash = signed_json
            .payload
            .get("content_hash")
            .and_then(|v| v.as_str())
            .ok_or((
                axum::http::StatusCode::BAD_REQUEST,
                "signed_json.payload.content_hashが見つかりません".to_string(),
            ))?;

        // Bubblegum V2 MintV2 トランザクション構築（仕様書 §5.1 Step 9-10）
        let mut tx = solana_tx::build_mint_v2_tx(
            &tree_pubkey,
            &tee_signing_pubkey,
            &creator_wallet,
            content_hash,
            &item.signed_json_uri,
            collection_mint,
            &blockhash,
        );

        // Step 4: TEE秘密鍵で部分署名
        let message_bytes = tx.message.serialize();
        let tee_sig = state.runtime.sign(&message_bytes);

        solana_tx::apply_partial_signature(&mut tx, &tee_signing_pubkey, &tee_sig).map_err(
            |e| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("TEE署名の適用に失敗: {e}"),
                )
            },
        )?;

        // Step 5: 部分署名済みトランザクションを返却
        let tx_bytes = solana_tx::serialize_transaction(&tx).map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("トランザクションのシリアライズに失敗: {e}"),
            )
        })?;

        partial_txs.push(b64().encode(&tx_bytes));
    }

    Ok(Json(SignResponse { partial_txs }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::mock::MockRuntime;
    use crate::runtime::TeeRuntime;
    use base58::ToBase58;
    use title_types::{Attribute, SignedJson, SignedJsonCore};
    use tokio::sync::{RwLock, Semaphore};

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
        let tee_pubkey_b58 = rt.signing_pubkey().to_base58();
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

    /// テスト用モックストレージを起動し、指定のデータを返す
    async fn start_mock_storage(data: Vec<u8>) -> u16 {
        use axum::routing::get;

        let app = axum::Router::new().route(
            "/signed_json",
            get(move || {
                let d = data.clone();
                async move { d }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        port
    }

    /// テスト用インラインプロキシを起動する
    async fn start_inline_proxy() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf4 = [0u8; 4];

                    // Read method
                    stream.read_exact(&mut buf4).await.unwrap();
                    let method_len = u32::from_be_bytes(buf4) as usize;
                    let mut method_buf = vec![0u8; method_len];
                    stream.read_exact(&mut method_buf).await.unwrap();
                    let method = String::from_utf8(method_buf).unwrap();

                    // Read url
                    stream.read_exact(&mut buf4).await.unwrap();
                    let url_len = u32::from_be_bytes(buf4) as usize;
                    let mut url_buf = vec![0u8; url_len];
                    stream.read_exact(&mut url_buf).await.unwrap();
                    let url = String::from_utf8(url_buf).unwrap();

                    // Read body
                    stream.read_exact(&mut buf4).await.unwrap();
                    let body_len = u32::from_be_bytes(buf4) as usize;
                    let mut body = vec![0u8; body_len];
                    if body_len > 0 {
                        stream.read_exact(&mut body).await.unwrap();
                    }

                    let client = reqwest::Client::new();
                    let result = match method.as_str() {
                        "GET" => client.get(&url).send().await,
                        "POST" => client.post(&url).body(body).send().await,
                        _ => {
                            stream.write_all(&400u32.to_be_bytes()).await.unwrap();
                            let msg = b"Unsupported method";
                            stream
                                .write_all(&(msg.len() as u32).to_be_bytes())
                                .await
                                .unwrap();
                            stream.write_all(msg).await.unwrap();
                            return;
                        }
                    };

                    match result {
                        Ok(resp) => {
                            let status = resp.status().as_u16() as u32;
                            let resp_body = resp.bytes().await.unwrap_or_default();
                            stream.write_all(&status.to_be_bytes()).await.unwrap();
                            stream
                                .write_all(&(resp_body.len() as u32).to_be_bytes())
                                .await
                                .unwrap();
                            stream.write_all(&resp_body).await.unwrap();
                        }
                        Err(_) => {
                            stream.write_all(&500u32.to_be_bytes()).await.unwrap();
                            let msg = b"Proxy error";
                            stream
                                .write_all(&(msg.len() as u32).to_be_bytes())
                                .await
                                .unwrap();
                            stream.write_all(msg).await.unwrap();
                        }
                    }
                });
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        port
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
        let storage_port = start_mock_storage(signed_json_bytes).await;
        let proxy_port = start_inline_proxy().await;

        // tree_addressを設定（create_tree済みの状態をシミュレート）
        let tree_pubkey_bytes: [u8; 32] = rt.tree_pubkey().try_into().unwrap();

        let state = Arc::new(AppState {
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

        let storage_port = start_mock_storage(signed_json_bytes).await;
        let proxy_port = start_inline_proxy().await;

        let tree_pubkey_bytes: [u8; 32] = new_rt.tree_pubkey().try_into().unwrap();

        let state = Arc::new(AppState {
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
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
        assert!(msg.contains("tee_signatureの検証に失敗"));
    }

    /// サイズ制限を超えるsigned_jsonが拒否されることを確認
    #[tokio::test]
    async fn test_sign_rejects_oversized() {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();
        rt.generate_tree_keypair();

        // 1MBを超えるデータを返すモックストレージ
        let oversized_data = vec![0u8; crate::security::MAX_SIGNED_JSON_SIZE as usize + 1];

        let storage_port = start_mock_storage(oversized_data).await;
        let proxy_port = start_inline_proxy().await;

        let tree_pubkey_bytes: [u8; 32] = rt.tree_pubkey().try_into().unwrap();

        let state = Arc::new(AppState {
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
        let (status, _msg) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// inactive状態での/sign呼び出しが503を返すことを確認
    #[tokio::test]
    async fn test_sign_inactive_returns_503() {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();
        rt.generate_tree_keypair();

        let state = Arc::new(AppState {
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
        let (status, _) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
