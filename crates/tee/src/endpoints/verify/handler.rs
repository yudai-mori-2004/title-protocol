// SPDX-License-Identifier: Apache-2.0

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
    EncryptedResponse, ProcessorResult, VerifyRequest, VerifyResponse,
};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::infra::security::{self, SecurityError};

use super::{detect_mime_type, CORE_PROCESSOR_ID};
use crate::endpoints::b64;

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
    // ダウンロード全体にグローバルタイムアウトを適用（チャンクタイムアウト積算によるSlowloris対策）
    let download_timeout =
        security::compute_dynamic_timeout(&limits, limits.max_single_content_bytes);
    let (proxy_response, _download_ticket) = tokio::time::timeout(
        download_timeout,
        security::proxy_get_secured(
            &state.proxy_addr,
            &request.download_url,
            limits.max_single_content_bytes,
            chunk_timeout,
            &state.resource_pool,
        ),
    )
    .await
    .map_err(|_| TeeError::Timeout)?
    .map_err(|e| match &e {
        SecurityError::PayloadTooLarge { .. } => TeeError::PayloadTooLarge(e.to_string()),
        SecurityError::MemoryLimitExceeded => TeeError::ServiceUnavailable(e.to_string()),
        SecurityError::ChunkReadTimeout { .. } => TeeError::Timeout,
        SecurityError::ProxyError(status) => {
            TeeError::BadGateway(format!("Temporary Storageがエラーを返しました: HTTP {status}"))
        }
        _ => TeeError::BadGateway(format!("暗号化ペイロードの取得に失敗: {e}")),
    })?;

    // Step 4. バイナリペイロード復号（ECDH + HKDF + AES-GCM）
    // 仕様書 §5.1 Step 2, §6.4 ハイブリッド暗号化 Step 6-7
    //
    // ワイヤーフォーマット: [32B: eph_pk][12B: nonce][remaining: ciphertext]
    let (eph_pubkey_arr, nonce, ciphertext) =
        title_types::parse_encrypted_payload(&proxy_response.body)
            .map_err(|e| TeeError::BadRequest(e))?;
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

    // AES-GCM復号
    let plaintext = title_crypto::aes_gcm_decrypt(&symmetric_key, &nonce, ciphertext)
        .map_err(|e| TeeError::BadRequest(format!("ペイロードの復号に失敗: {e}")))?;
    drop(proxy_response); // ダウンロードデータのメモリを早期解放

    // 平文フォーマット: [4B: metadata_len][metadata JSON][raw content bytes]
    let (client_metadata, content_offset) =
        title_types::parse_plaintext_payload(&plaintext)
            .map_err(|e| TeeError::BadRequest(e))?;
    let content_bytes = plaintext[content_offset..].to_vec();
    drop(plaintext); // 平文バッファのメモリを早期解放

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

    // Step 5. processor_idsに基づくCore/Extension並列実行（タイムアウト付き）
    // 仕様書 §5.1 Step 4-5
    // 各プロセッサはステートレスに独立動作し、並列実行する。
    let processing_result = tokio::time::timeout(global_timeout, async {
        // Extension IDの事前検証（不正IDは早期エラー）
        if let Some(ref trusted) = state.trusted_extension_ids {
            for processor_id in &request.processor_ids {
                if processor_id != CORE_PROCESSOR_ID && !trusted.contains(processor_id.as_str()) {
                    return Err(TeeError::Forbidden(format!(
                        "信頼されていないExtension IDです: {processor_id}。\
                         TRUSTED_EXTENSIONS環境変数で許可してください"
                    )));
                }
            }
        }

        // 全プロセッサを並列起動
        let mut handles = Vec::new();
        for processor_id in &request.processor_ids {
            let state = std::sync::Arc::clone(&state);
            let content = content_bytes.clone();
            let mime = mime_type.to_string();
            let wallet = client_metadata.owner_wallet.clone();
            let pid = processor_id.clone();
            let ext_inputs = client_metadata.extension_inputs.clone();
            let max_graph = limits.c2pa_max_graph_size;

            let handle = tokio::spawn(async move {
                let signed_json = if pid == CORE_PROCESSOR_ID {
                    let sj = super::core::process_core(
                        &state, &content, &mime, &wallet, max_graph,
                    )
                    .map_err(|e| format!("Core処理に失敗: {e}"))?;
                    serde_json::to_value(&sj)
                        .map_err(|e| format!("signed_jsonのシリアライズに失敗: {e}"))?
                } else {
                    super::extension::process_extension(
                        &state, &content, &mime, &wallet, &pid,
                        ext_inputs.as_ref().and_then(|m| m.get(&pid)),
                    )
                    .await
                    .map_err(|e| format!("Extension処理に失敗 ({}): {e}", pid))?
                };
                Ok::<ProcessorResult, String>(ProcessorResult {
                    processor_id: pid,
                    signed_json,
                })
            });
            handles.push(handle);
        }

        // 全結果を収集（順序保持）
        let mut results = Vec::new();
        for handle in handles {
            let result = handle
                .await
                .map_err(|e| TeeError::Internal(format!("プロセッサタスクエラー: {e}")))?
                .map_err(TeeError::ProcessingFailed)?;
            results.push(result);
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
