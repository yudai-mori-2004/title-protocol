//! # /verify メインハンドラ
//!
//! 仕様書 §1.1 Phase 1, §6.4
//!
//! ## 処理フロー
//! 1. Gateway署名を検証
//! 2. resource_limitsを適用
//! 3. download_urlから暗号化ペイロードを取得
//! 4. ペイロードを復号（ハイブリッド暗号化の逆操作）
//! 5. processor_idsに基づきCore/Extension処理を実行
//! 6. レスポンスを暗号化して返却

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use title_types::{
    EncryptedPayload, EncryptedResponse, ProcessorResult, VerifyRequest, VerifyResponse,
};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::infra::security::{self, SecurityError};

use super::{b64, detect_mime_type, CORE_PROCESSOR_ID};

/// /verify エンドポイントハンドラ。
/// 仕様書 §1.1 Phase 1, §6.4
pub async fn handle_verify(
    State(state): State<Arc<TeeAppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<EncryptedResponse>, TeeError> {
    // active状態チェック
    {
        let current = state.state.read().await;
        if *current != TeeState::Active {
            return Err(TeeError::InvalidState("TEEはまだactive状態ではありません".into()));
        }
    }

    // Step 1. Gateway署名の検証（§6.2）
    let (inner_body, resource_limits) =
        crate::infra::gateway_auth::verify_gateway_auth(state.gateway_pubkey.as_ref(), &body)
            .map_err(|(_, msg)| TeeError::Unauthorized(msg))?;

    let request: VerifyRequest = serde_json::from_value(inner_body)
        .map_err(|e| TeeError::BadRequest(format!("VerifyRequestのパースに失敗: {e}")))?;

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
        SecurityError::PayloadTooLarge { .. } => TeeError::PayloadTooLarge(e.to_string()),
        SecurityError::MemoryLimitExceeded => TeeError::ServiceUnavailable(e.to_string()),
        SecurityError::ChunkReadTimeout { .. } => TeeError::Timeout,
        SecurityError::ProxyError(status) => {
            TeeError::BadGateway(format!("Temporary Storageがエラーを返しました: HTTP {status}"))
        }
        _ => TeeError::BadGateway(format!("暗号化ペイロードの取得に失敗: {e}")),
    })?;

    let encrypted_payload: EncryptedPayload =
        serde_json::from_slice(&proxy_response.body)
            .map_err(|e| TeeError::BadGateway(format!("暗号化ペイロードのパースに失敗: {e}")))?;

    // Step 4. ペイロード復号（ECDH + HKDF + AES-GCM）
    // 仕様書 §6.4 ハイブリッド暗号化 Step 6-7
    let eph_pubkey_bytes = b64()
        .decode(&encrypted_payload.ephemeral_pubkey)
        .map_err(|e| TeeError::BadRequest(format!("ephemeral_pubkeyのBase64デコードに失敗: {e}")))?;
    let eph_pubkey_arr: [u8; 32] = eph_pubkey_bytes.try_into()
        .map_err(|_| TeeError::BadRequest("ephemeral_pubkeyは32バイトである必要があります".into()))?;
    let eph_pubkey = X25519PublicKey::from(eph_pubkey_arr);

    let tee_secret_bytes: [u8; 32] = state
        .runtime
        .encryption_secret_key()
        .try_into()
        .map_err(|_| TeeError::Internal("暗号化用秘密鍵の取得に失敗".into()))?;
    let tee_secret = StaticSecret::from(tee_secret_bytes);

    // ECDH(tee_sk, eph_pk) → shared_secret
    let shared_secret = title_crypto::ecdh_derive_shared_secret(&tee_secret, &eph_pubkey);
    // HKDF → symmetric_key
    let symmetric_key = title_crypto::hkdf_derive_key(&shared_secret)
        .map_err(|e| TeeError::Internal(format!("対称鍵の導出に失敗: {e}")))?;

    let nonce_bytes = b64().decode(&encrypted_payload.nonce)
        .map_err(|e| TeeError::BadRequest(format!("nonceのBase64デコードに失敗: {e}")))?;
    let nonce: [u8; 12] = nonce_bytes.try_into()
        .map_err(|_| TeeError::BadRequest("nonceは12バイトである必要があります".into()))?;

    let ciphertext = b64()
        .decode(&encrypted_payload.ciphertext)
        .map_err(|e| TeeError::BadRequest(format!("ciphertextのBase64デコードに失敗: {e}")))?;

    // AES-GCM復号
    let plaintext = title_crypto::aes_gcm_decrypt(&symmetric_key, &nonce, &ciphertext)
        .map_err(|e| TeeError::BadRequest(format!("ペイロードの復号に失敗: {e}")))?;

    // ClientPayloadをパース
    let client_payload: title_types::ClientPayload = serde_json::from_slice(&plaintext)
        .map_err(|e| TeeError::BadRequest(format!("ClientPayloadのパースに失敗: {e}")))?;

    // コンテンツをBase64デコード
    let content_bytes = b64().decode(&client_payload.content)
        .map_err(|e| TeeError::BadRequest(format!("contentのBase64デコードに失敗: {e}")))?;

    // MIMEタイプを検出
    let mime_type = detect_mime_type(&content_bytes);

    // コンテンツサイズの事後検証（復号後の実データサイズ）
    // 仕様書 §6.4
    if content_bytes.len() as u64 > limits.max_single_content_bytes {
        return Err(TeeError::PayloadTooLarge(format!(
            "コンテンツサイズが上限を超えています: {} bytes (上限: {} bytes)",
            content_bytes.len(),
            limits.max_single_content_bytes
        )));
    }

    // 動的グローバルタイムアウト適用（仕様書 §6.4）
    let global_timeout = security::compute_dynamic_timeout(&limits, content_bytes.len() as u64);

    // Step 5. processor_idsに基づくCore/Extension実行（タイムアウト付き）
    // 仕様書 §5.1 Step 4-5
    let processing_result = tokio::time::timeout(global_timeout, async {
        let mut results = Vec::new();

        for processor_id in &request.processor_ids {
            if processor_id == CORE_PROCESSOR_ID {
                // Core: C2PA検証 + 来歴グラフ構築
                let signed_json = super::core::process_core(
                    &state,
                    &content_bytes,
                    mime_type,
                    &client_payload.owner_wallet,
                    limits.c2pa_max_graph_size,
                )
                .map_err(|e| TeeError::ProcessingFailed(format!("Core処理に失敗: {e}")))?;

                results.push(ProcessorResult {
                    processor_id: processor_id.clone(),
                    signed_json: serde_json::to_value(&signed_json)
                        .map_err(|e| TeeError::Internal(format!("signed_jsonのシリアライズに失敗: {e}")))?,
                });
            } else {
                // Extension: WASM実行
                // 仕様書 §6.4 不正WASMインジェクション防御
                if let Some(ref trusted) = state.trusted_extension_ids {
                    if !trusted.contains(processor_id.as_str()) {
                        return Err(TeeError::Forbidden(format!(
                            "信頼されていないExtension IDです: {processor_id}。\
                             TRUSTED_EXTENSIONS環境変数で許可してください"
                        )));
                    }
                }

                // 仕様書 §5.1 Step 5, §7.1
                let signed_json = super::extension::process_extension(
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
                .map_err(|e| TeeError::ProcessingFailed(format!("Extension処理に失敗 ({}): {e}", processor_id)))?;

                results.push(ProcessorResult {
                    processor_id: processor_id.clone(),
                    signed_json,
                });
            }
        }

        Ok::<Vec<ProcessorResult>, TeeError>(results)
    })
    .await
    .map_err(|_| TeeError::Timeout)?;

    let results = processing_result?;

    // Step 7. レスポンスを共通鍵で暗号化して返却
    // 仕様書 §5.1 Step 6, §6.4
    let verify_response = VerifyResponse { results };
    let response_json = serde_json::to_vec(&verify_response)
        .map_err(|e| TeeError::Internal(format!("VerifyResponseのシリアライズに失敗: {e}")))?;

    // 新しいnonceを生成
    let mut response_nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut response_nonce);

    // 同一symmetric_key、新しいnonceでAES-GCM暗号化
    let response_ciphertext =
        title_crypto::aes_gcm_encrypt(&symmetric_key, &response_nonce, &response_json)
            .map_err(|e| TeeError::Internal(format!("レスポンスの暗号化に失敗: {e}")))?;

    let encrypted_response = EncryptedResponse {
        nonce: b64().encode(response_nonce),
        ciphertext: b64().encode(response_ciphertext),
    };

    Ok(Json(encrypted_response))
}
