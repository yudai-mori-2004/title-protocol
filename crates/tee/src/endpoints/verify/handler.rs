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
use base64::engine::general_purpose::STANDARD as B64;

use title_types::{
    EncryptedResponse, ProcessorResult, VerifyRequest, VerifyResponse,
};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::infra::security::{self, SecurityError};

use super::{detect_mime_type, CORE_PROCESSOR_ID};


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
        crate::infra::gateway_auth::verify_gateway_auth(state.gateway_pubkey.as_deref(), &body)
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
    let (proxy_response, download_ticket) = tokio::time::timeout(
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

    // Step 4. バイナリペイロード復号
    // 仕様書 §6.4 open_request(): KEM + KDF + AEAD を内部で合成
    let payload_size = proxy_response.body.len();
    if !download_ticket.extend(payload_size) {
        return Err(TeeError::ServiceUnavailable(
            "メモリ予算超過: 復号バッファを確保できません".into(),
        ));
    }
    let opened = title_crypto::open_request(
        state.runtime.decapsulator(),
        &proxy_response.body,
        b"/verify",
    )
    .map_err(|e| TeeError::BadRequest(format!("ペイロードの復号に失敗: {e}")))?;
    let plaintext = opened.plaintext;
    drop(proxy_response);
    download_ticket.shrink(payload_size);

    // 平文フォーマット: [4B: metadata_len][metadata JSON][raw content bytes]
    let (client_metadata, content_offset) =
        title_types::parse_plaintext_payload(&plaintext)
            .map_err(|e| TeeError::BadRequest(e))?;
    let plaintext_size = plaintext.len();
    let content_size = plaintext_size - content_offset;
    if !download_ticket.extend(content_size) {
        return Err(TeeError::ServiceUnavailable(
            "メモリ予算超過: コンテンツバッファを確保できません".into(),
        ));
    }
    let content_bytes = plaintext[content_offset..].to_vec();
    drop(plaintext);
    download_ticket.shrink(plaintext_size); // 平文バッファ解放分を返却

    // MIMEタイプを検出
    let mime_type = detect_mime_type(&content_bytes);

    // processor並列実行で content_bytes がcloneされる分を事前予約。
    // clone数 = processor_ids.len() (元のcontent_bytesは別途保持される)
    let clone_overhead = content_bytes.len() * request.processor_ids.len();
    if !download_ticket.extend(clone_overhead) {
        return Err(TeeError::ServiceUnavailable(
            "メモリ予算超過: 並列処理用メモリを確保できません".into(),
        ));
    }

    // TicketをArcでラップし、各processorのWasmRunnerと共有する。
    // decode/Jarosz等のメモリ確保はホスト関数内でextend/shrinkされる。
    // handler終了時にArcの最後の参照がdrop → Ticket drop → 全予約解放。
    let request_ticket = std::sync::Arc::new(download_ticket);

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
            let ticket = std::sync::Arc::clone(&request_ticket);

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
                        ticket,
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

    // Step 7. レスポンスをResponseSealerで暗号化して返却
    // 仕様書 §6.4: response_keyで暗号化（request_keyとは独立）
    let verify_response = VerifyResponse { results };
    let response_json = serde_json::to_vec(&verify_response)
        .map_err(|e| TeeError::Internal(format!("VerifyResponseのシリアライズに失敗: {e}")))?;

    // ResponseSealer.seal(): nonce生成 + AEAD暗号化。戻り値は nonce || ciphertext
    let sealed = opened.channel
        .seal(&response_json, b"/verify")
        .map_err(|e| TeeError::Internal(format!("レスポンスの暗号化に失敗: {e}")))?;

    // nonce(12B) || ciphertext に分割してBase64エンコード
    let nonce_size = 12; // AES-256-GCM
    let encrypted_response = EncryptedResponse {
        nonce: B64.encode(&sealed[..nonce_size]),
        ciphertext: B64.encode(&sealed[nonce_size..]),
    };

    Ok(Json(encrypted_response))
}
