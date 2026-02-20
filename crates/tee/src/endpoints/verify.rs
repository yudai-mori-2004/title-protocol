//! # /verify エンドポイント
//!
//! 仕様書 §6.4 /verifyフェーズの内部処理
//!
//! ## 処理フロー
//! 1. Gateway署名を検証（Global Configのgateway_pubkeyを使用）
//! 2. resource_limitsが含まれていれば適用、なければデフォルト値を使用
//! 3. download_urlからTemporary Storage上の暗号化ペイロードを取得
//! 4. ペイロードを復号（ハイブリッド暗号化の逆操作）
//! 5. processor_idsに基づき、Core（C2PA検証＋来歴グラフ構築）およびExtension（WASM実行）を処理
//! 6. 検証結果をJSON形式でまとめ、TEE秘密鍵で署名（tee_signature）
//! 7. signed_jsonを共通鍵と新しいnonceでAES-GCM暗号化し返却

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use base58::ToBase58;
use base64::Engine;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use title_types::{
    Attribute, CorePayload, EncryptedPayload, EncryptedResponse, ExtensionPayload,
    ProcessorResult, SignedJson, SignedJsonCore, VerifyRequest, VerifyResponse,
};

use crate::security::{self, SecurityError};
use crate::{AppState, TeeState};

/// Base64エンジン（Standard）
fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// コンテンツのMIMEタイプをマジックバイトから検出する。
/// 仕様書 §2.1
fn detect_mime_type(data: &[u8]) -> &str {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.len() >= 12 && data[8..12] == *b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Core プロセッサID。
const CORE_PROCESSOR_ID: &str = "core-c2pa";

/// /verify エンドポイントハンドラ。
/// 仕様書 §1.1 Phase 1, §6.4
pub async fn handle_verify(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<EncryptedResponse>, (axum::http::StatusCode, String)> {
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
    // GatewayAuthWrapperの gateway_signature をGlobal Configの gateway_pubkey で検証する。
    let (inner_body, resource_limits) =
        crate::gateway_auth::verify_gateway_auth(state.gateway_pubkey.as_ref(), &body)?;

    let request: VerifyRequest = serde_json::from_value(inner_body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("VerifyRequestのパースに失敗: {e}"),
        )
    })?;

    // Step 2. resource_limitsの完全適用（§6.4 処理上限の管理）
    let limits = security::resolve_limits(resource_limits.as_ref());
    let chunk_timeout = Duration::from_secs(limits.chunk_read_timeout_sec);

    // Step 3. download_urlからプロキシ経由で暗号化ペイロードを取得
    // 仕様書 §5.1 Step 3, §6.4
    // 三層防御: Zip Bomb対策 + Reservation DoS対策 + Slowloris対策
    let proxy_response = security::proxy_get_secured(
        &state.proxy_addr,
        &request.download_url,
        limits.max_single_content_bytes,
        chunk_timeout,
        &state.memory_semaphore,
    )
    .await
    .map_err(|e| match &e {
        SecurityError::PayloadTooLarge { .. } => (
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            e.to_string(),
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
            format!("Temporary Storageがエラーを返しました: HTTP {status}"),
        ),
        _ => (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("暗号化ペイロードの取得に失敗: {e}"),
        ),
    })?;

    let encrypted_payload: EncryptedPayload =
        serde_json::from_slice(&proxy_response.body).map_err(|e| {
            (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("暗号化ペイロードのパースに失敗: {e}"),
            )
        })?;

    // Step 4. ペイロード復号（ECDH + HKDF + AES-GCM）
    // 仕様書 §6.4 ハイブリッド暗号化 Step 6-7
    let eph_pubkey_bytes = b64()
        .decode(&encrypted_payload.ephemeral_pubkey)
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("ephemeral_pubkeyのBase64デコードに失敗: {e}"),
            )
        })?;
    let eph_pubkey_arr: [u8; 32] = eph_pubkey_bytes.try_into().map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "ephemeral_pubkeyは32バイトである必要があります".to_string(),
        )
    })?;
    let eph_pubkey = X25519PublicKey::from(eph_pubkey_arr);

    let tee_secret_bytes: [u8; 32] = state
        .runtime
        .encryption_secret_key()
        .try_into()
        .map_err(|_| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "暗号化用秘密鍵の取得に失敗".to_string(),
            )
        })?;
    let tee_secret = StaticSecret::from(tee_secret_bytes);

    // ECDH(tee_sk, eph_pk) → shared_secret
    let shared_secret = title_crypto::ecdh_derive_shared_secret(&tee_secret, &eph_pubkey);
    // HKDF → symmetric_key
    let symmetric_key = title_crypto::hkdf_derive_key(&shared_secret).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("対称鍵の導出に失敗: {e}"),
        )
    })?;

    let nonce_bytes = b64().decode(&encrypted_payload.nonce).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("nonceのBase64デコードに失敗: {e}"),
        )
    })?;
    let nonce: [u8; 12] = nonce_bytes.try_into().map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "nonceは12バイトである必要があります".to_string(),
        )
    })?;

    let ciphertext = b64()
        .decode(&encrypted_payload.ciphertext)
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("ciphertextのBase64デコードに失敗: {e}"),
            )
        })?;

    // AES-GCM復号
    let plaintext = title_crypto::aes_gcm_decrypt(&symmetric_key, &nonce, &ciphertext).map_err(
        |e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("ペイロードの復号に失敗: {e}"),
            )
        },
    )?;

    // ClientPayloadをパース
    let client_payload: title_types::ClientPayload =
        serde_json::from_slice(&plaintext).map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("ClientPayloadのパースに失敗: {e}"),
            )
        })?;

    // コンテンツをBase64デコード
    let content_bytes = b64().decode(&client_payload.content).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("contentのBase64デコードに失敗: {e}"),
        )
    })?;

    // MIMEタイプを検出
    let mime_type = detect_mime_type(&content_bytes);

    // コンテンツサイズの事後検証（復号後の実データサイズ）
    // 仕様書 §6.4
    if content_bytes.len() as u64 > limits.max_single_content_bytes {
        return Err((
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "コンテンツサイズが上限を超えています: {} bytes (上限: {} bytes)",
                content_bytes.len(),
                limits.max_single_content_bytes
            ),
        ));
    }

    // 動的グローバルタイムアウト適用（仕様書 §6.4）
    // Timeout = min(MaxLimit, BaseTime + ContentSize / MinSpeed)
    let global_timeout = security::compute_dynamic_timeout(&limits, content_bytes.len() as u64);

    // Step 5. processor_idsに基づくCore/Extension実行（タイムアウト付き）
    // 仕様書 §5.1 Step 4-5
    let processing_result = tokio::time::timeout(global_timeout, async {
        let mut results = Vec::new();

        for processor_id in &request.processor_ids {
            if processor_id == CORE_PROCESSOR_ID {
                // Core: C2PA検証 + 来歴グラフ構築
                let signed_json = process_core(
                    &state,
                    &content_bytes,
                    mime_type,
                    &client_payload.owner_wallet,
                    limits.c2pa_max_graph_size,
                )
                .map_err(|e| {
                    (
                        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                        format!("Core処理に失敗: {e}"),
                    )
                })?;

                results.push(ProcessorResult {
                    processor_id: processor_id.clone(),
                    signed_json: serde_json::to_value(&signed_json).map_err(|e| {
                        (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("signed_jsonのシリアライズに失敗: {e}"),
                        )
                    })?,
                });
            } else {
                // Extension: WASM実行
                // 仕様書 §6.4 不正WASMインジェクション防御
                // trusted_extension_idsが設定されている場合、extension_idが一覧に存在するか確認
                if let Some(ref trusted) = state.trusted_extension_ids {
                    if !trusted.contains(processor_id.as_str()) {
                        return Err((
                            axum::http::StatusCode::FORBIDDEN,
                            format!(
                                "信頼されていないExtension IDです: {processor_id}。\
                                 TRUSTED_EXTENSIONS環境変数で許可してください"
                            ),
                        ));
                    }
                }

                // 仕様書 §5.1 Step 5, §7.1
                let signed_json = process_extension(
                    &state,
                    &content_bytes,
                    mime_type,
                    &client_payload.owner_wallet,
                    processor_id,
                    client_payload
                        .extension_inputs
                        .as_ref()
                        .and_then(|m| m.get(processor_id)),
                )
                .await
                .map_err(|e| {
                    (
                        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                        format!("Extension処理に失敗 ({}): {e}", processor_id),
                    )
                })?;

                results.push(ProcessorResult {
                    processor_id: processor_id.clone(),
                    signed_json,
                });
            }
        }

        Ok::<Vec<ProcessorResult>, (axum::http::StatusCode, String)>(results)
    })
    .await
    .map_err(|_| {
        (
            axum::http::StatusCode::REQUEST_TIMEOUT,
            "リクエスト処理がタイムアウトしました".to_string(),
        )
    })?;

    let results = processing_result?;

    // Step 7. レスポンスを共通鍵で暗号化して返却
    // 仕様書 §5.1 Step 6, §6.4
    let verify_response = VerifyResponse { results };
    let response_json = serde_json::to_vec(&verify_response).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("VerifyResponseのシリアライズに失敗: {e}"),
        )
    })?;

    // 新しいnonceを生成
    let mut response_nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut response_nonce);

    // 同一symmetric_key、新しいnonceでAES-GCM暗号化
    let response_ciphertext =
        title_crypto::aes_gcm_encrypt(&symmetric_key, &response_nonce, &response_json).map_err(
            |e| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("レスポンスの暗号化に失敗: {e}"),
                )
            },
        )?;

    let encrypted_response = EncryptedResponse {
        nonce: b64().encode(response_nonce),
        ciphertext: b64().encode(response_ciphertext),
    };

    Ok(Json(encrypted_response))
}

/// Core処理: C2PA検証 + 来歴グラフ構築 + signed_json生成。
/// 仕様書 §2.1, §2.2, §5.1 Step 4
fn process_core(
    state: &AppState,
    content_bytes: &[u8],
    mime_type: &str,
    owner_wallet: &str,
    max_graph_size: usize,
) -> Result<SignedJson, String> {
    // C2PA検証
    let c2pa_result = title_core::verify_c2pa(content_bytes, mime_type)
        .map_err(|e| format!("C2PA検証エラー: {e}"))?;

    // content_hash計算
    let content_hash =
        title_crypto::content_hash_from_manifest_signature(&c2pa_result.active_manifest_signature);
    let content_hash_hex = format_content_hash(&content_hash);

    // 来歴グラフ構築
    let graph = title_core::build_provenance_graph(content_bytes, mime_type, max_graph_size)
        .map_err(|e| format!("来歴グラフ構築エラー: {e}"))?;

    // CorePayload構築
    let payload = CorePayload {
        content_hash: content_hash_hex.clone(),
        content_type: c2pa_result.content_type.clone(),
        creator_wallet: owner_wallet.to_string(),
        tsa_timestamp: c2pa_result.tsa_timestamp,
        tsa_pubkey_hash: c2pa_result.tsa_pubkey_hash.clone(),
        tsa_token_data: c2pa_result
            .tsa_token_data
            .as_ref()
            .map(|d| b64().encode(d)),
        nodes: graph.nodes,
        links: graph.links,
    };

    // attributes構築（cNFTオンチェーンメタデータ用）
    // 仕様書 §5.1 Step 4
    let attributes = vec![
        Attribute {
            trait_type: "protocol".to_string(),
            value: "Title-v1".to_string(),
        },
        Attribute {
            trait_type: "content_hash".to_string(),
            value: content_hash_hex,
        },
        Attribute {
            trait_type: "content_type".to_string(),
            value: c2pa_result.content_type,
        },
    ];

    // Step 6. signed_json構築 + TEE秘密鍵で署名（tee_signature）
    // 仕様書 §5.1 Step 4
    let payload_value = serde_json::to_value(&payload).map_err(|e| format!("payloadシリアライズエラー: {e}"))?;
    let attributes_value =
        serde_json::to_value(&attributes).map_err(|e| format!("attributesシリアライズエラー: {e}"))?;

    // 署名対象: payload + attributes の正規化JSON
    let sign_target = serde_json::json!({
        "payload": payload_value,
        "attributes": attributes_value,
    });
    let sign_bytes =
        serde_json::to_vec(&sign_target).map_err(|e| format!("署名対象のシリアライズエラー: {e}"))?;

    // TEE秘密鍵で署名
    let signature = state.runtime.sign(&sign_bytes);

    // TEE公開鍵（Base58エンコード）
    let tee_pubkey_b58 = state.runtime.signing_pubkey().to_base58();

    // Attestation Document（Base64エンコード）
    let attestation = state.runtime.get_attestation();
    let attestation_b64 = b64().encode(&attestation);

    // signed_json組み立て
    let signed_json = SignedJson {
        core: SignedJsonCore {
            protocol: "Title-v1".to_string(),
            tee_type: state.runtime.tee_type().to_string(),
            tee_pubkey: tee_pubkey_b58,
            tee_signature: b64().encode(&signature),
            tee_attestation: attestation_b64,
        },
        payload: payload_value,
        attributes,
    };

    Ok(signed_json)
}

/// Extension処理: WASM実行 + Extension signed_json生成。
/// 仕様書 §3.1, §5.1 Step 5, §7.1
///
/// WASMバイナリはWasmLoaderトレイト経由で取得する。
/// エクスポート関数名は標準化された `process` を使用する。
async fn process_extension(
    state: &AppState,
    content_bytes: &[u8],
    mime_type: &str,
    owner_wallet: &str,
    extension_id: &str,
    extension_input: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    // WASMローダーを取得
    let loader = state
        .wasm_loader
        .as_ref()
        .ok_or_else(|| "WASMローダーが設定されていません。Extension実行には WASM_DIR または WASM_BASE_URL の設定が必要です".to_string())?;

    // WASMバイナリをロード（ファイルまたはHTTP経由）
    let wasm_binary = loader.load(extension_id).await?;

    // WASMバイナリのSHA-256ハッシュを計算
    let wasm_hash = title_crypto::content_hash_from_manifest_signature(&wasm_binary.bytes);
    let wasm_hash_hex = format_content_hash(&wasm_hash);

    // Extension補助入力をシリアライズ
    let ext_input_bytes = extension_input
        .map(|v| serde_json::to_vec(v))
        .transpose()
        .map_err(|e| format!("extension_inputのシリアライズに失敗: {e}"))?;

    // extension_inputのハッシュ（存在する場合）
    let ext_input_hash = ext_input_bytes.as_ref().map(|bytes| {
        let hash = title_crypto::content_hash_from_manifest_signature(bytes);
        format_content_hash(&hash)
    });

    // WASMランナーで実行（仕様書 §7.1）
    // 標準エクスポート関数名 "process" を使用
    let runner = title_wasm_host::WasmRunner::new(
        100_000_000, // Fuel制限: 1億命令
        64 * 1024 * 1024, // Memory制限: 64MB
    );

    let wasm_result = runner
        .execute(
            &wasm_binary.bytes,
            content_bytes,
            ext_input_bytes.as_deref(),
            crate::wasm_loader::STANDARD_EXPORT_NAME,
        )
        .map_err(|e| format!("WASM実行エラー: {e}"))?;

    // content_hash計算（C2PA検証結果から取得）
    let c2pa_result = title_core::verify_c2pa(content_bytes, mime_type)
        .map_err(|e| format!("C2PA検証エラー: {e}"))?;
    let content_hash =
        title_crypto::content_hash_from_manifest_signature(&c2pa_result.active_manifest_signature);
    let content_hash_hex = format_content_hash(&content_hash);

    // ExtensionPayload構築（仕様書 §5.1 Step 5）
    let payload = ExtensionPayload {
        content_hash: content_hash_hex.clone(),
        content_type: mime_type.to_string(),
        creator_wallet: owner_wallet.to_string(),
        extension_id: extension_id.to_string(),
        wasm_source: wasm_binary.source.clone(),
        wasm_hash: wasm_hash_hex.clone(),
        extension_input_hash: ext_input_hash.clone(),
        result: wasm_result.output,
    };

    // attributes構築
    let attributes = vec![
        Attribute {
            trait_type: "protocol".to_string(),
            value: "Title-Extension-v1".to_string(),
        },
        Attribute {
            trait_type: "content_hash".to_string(),
            value: content_hash_hex.clone(),
        },
        Attribute {
            trait_type: "extension_id".to_string(),
            value: extension_id.to_string(),
        },
    ];

    // 署名対象の構築と署名（仕様書 §5.1 Step 5）
    let payload_value = serde_json::to_value(&payload)
        .map_err(|e| format!("payloadシリアライズエラー: {e}"))?;
    let attributes_value = serde_json::to_value(&attributes)
        .map_err(|e| format!("attributesシリアライズエラー: {e}"))?;

    let sign_target = serde_json::json!({
        "payload": payload_value,
        "attributes": attributes_value,
    });
    let sign_bytes = serde_json::to_vec(&sign_target)
        .map_err(|e| format!("署名対象のシリアライズエラー: {e}"))?;

    let signature = state.runtime.sign(&sign_bytes);
    let tee_pubkey_b58 = state.runtime.signing_pubkey().to_base58();
    let attestation_b64 = b64().encode(state.runtime.get_attestation());

    // Extension signed_json構築
    let signed_json = serde_json::json!({
        "protocol": "Title-Extension-v1",
        "tee_type": state.runtime.tee_type(),
        "tee_pubkey": tee_pubkey_b58,
        "tee_signature": b64().encode(&signature),
        "tee_attestation": attestation_b64,
        "content_hash": content_hash_hex,
        "content_type": mime_type,
        "creator_wallet": owner_wallet,
        "extension_id": extension_id,
        "wasm_source": wasm_binary.source,
        "wasm_hash": wasm_hash_hex,
        "extension_input_hash": ext_input_hash,
        "payload": payload_value,
        "attributes": attributes_value,
    });

    Ok(signed_json)
}

/// content_hashを「0x」プレフィックス付きhex文字列に変換する。
/// 仕様書 §2.1
fn format_content_hash(hash: &[u8; 32]) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("0x{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::mock::MockRuntime;
    use crate::runtime::TeeRuntime;
    use std::io::Cursor;
    use tokio::sync::{RwLock, Semaphore};

    // テストフィクスチャ（core crateから参照）
    const CERTS: &[u8] = include_bytes!("../../../core/tests/fixtures/certs/chain.pem");
    const PRIVATE_KEY: &[u8] = include_bytes!("../../../core/tests/fixtures/certs/ee.key");
    const TEST_IMAGE: &[u8] = include_bytes!("../../../core/tests/fixtures/test.jpg");

    /// テスト用signerを作成する（core crateのテストと同一パターン）
    fn test_signer() -> Box<dyn c2pa::Signer> {
        c2pa::settings::load_settings_from_str(
            r#"{"verify": {"verify_after_sign": false}}"#,
            "json",
        )
        .unwrap();
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

    /// テスト用モックTemporary Storageを起動し、指定のペイロードを /payload で返す
    async fn start_mock_temp_storage(payload_bytes: Vec<u8>) -> u16 {
        use axum::routing::get;

        let app = axum::Router::new().route(
            "/payload",
            get(move || {
                let data = payload_bytes.clone();
                async move { data }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // サーバー起動を待つ
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        port
    }

    /// テスト用インラインプロキシを起動する
    /// proxy crateのTCPフォールバックと同等のlength-prefixedプロトコルでHTTPリクエストを転送する
    async fn start_inline_proxy() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    // Read method
                    let mut buf4 = [0u8; 4];
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

                    // Forward via reqwest
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
        // サーバー起動を待つ
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        port
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
        let mock_port = start_mock_temp_storage(encrypted_payload_bytes).await;
        let proxy_port = start_inline_proxy().await;

        // 5. AppState構築
        let state = Arc::new(AppState {
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

        let mock_port = start_mock_temp_storage(encrypted_payload_bytes).await;
        let proxy_port = start_inline_proxy().await;

        // 3. AppState構築（wasm_dir指定あり）
        let state = Arc::new(AppState {
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
            "download_url": "http://example.com/payload",
            "processor_ids": ["core-c2pa"],
        });

        let result = handle_verify(State(state), Json(body)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
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

        let mock_port = start_mock_temp_storage(encrypted_payload_bytes).await;
        let proxy_port = start_inline_proxy().await;

        // trusted_extension_idsに "phash-v1" のみ許可（"evil-ext" は不許可）
        let mut trusted = std::collections::HashSet::new();
        trusted.insert("phash-v1".to_string());

        let state = Arc::new(AppState {
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
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
        assert!(
            msg.contains("信頼されていないExtension ID"),
            "エラーメッセージに '信頼されていないExtension ID' が含まれるべき: {msg}"
        );

        let _ = std::fs::remove_dir_all(&wasm_dir);
    }
}
