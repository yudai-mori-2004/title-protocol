//! /sign ハンドラ実装

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use ed25519_dalek::VerifyingKey;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use title_types::{SignRequest, SignResponse, SignedJson};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::infra::security::{self, SecurityError};
use crate::blockchain::solana_tx;

/// Base64エンジン（Standard）
pub(crate) fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// /sign エンドポイントハンドラ。
/// 仕様書 §1.1 Phase 2, §6.4
pub async fn handle_sign(
    State(state): State<Arc<TeeAppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<SignResponse>, TeeError> {
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

    let request: SignRequest = serde_json::from_value(inner_body)
        .map_err(|e| TeeError::BadRequest(format!("SignRequestのパースに失敗: {e}")))?;

    // resource_limitsの適用（§6.4）
    let limits = security::resolve_limits(resource_limits.as_ref());
    let chunk_timeout = Duration::from_secs(limits.chunk_read_timeout_sec);

    // recent_blockhash（Base58デコード）
    let blockhash = solana_sdk::hash::Hash::from_str(&request.recent_blockhash)
        .map_err(|e| TeeError::BadRequest(format!("recent_blockhashのBase58デコードに失敗: {e}")))?;

    // TEE署名用公開鍵
    let tee_pubkey_bytes: [u8; 32] = state.runtime.signing_pubkey().try_into()
        .map_err(|_| TeeError::Internal("署名用公開鍵の取得に失敗".into()))?;
    let tee_signing_pubkey = Pubkey::new_from_array(tee_pubkey_bytes);

    // Ed25519検証用キー
    let verifying_key = VerifyingKey::from_bytes(&tee_pubkey_bytes)
        .map_err(|e| TeeError::Internal(format!("検証用公開鍵の構築に失敗: {e}")))?;

    // Tree address
    let tree_address_bytes = {
        let tree_addr = state.tree_address.read().await;
        tree_addr.ok_or(TeeError::Internal(
            "Merkle Treeが未作成です。先に/create-treeを呼び出してください".into(),
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
            SecurityError::PayloadTooLarge { .. } => TeeError::PayloadTooLarge(format!("signed_jsonのサイズが上限を超えています: {e}")),
            SecurityError::MemoryLimitExceeded => TeeError::ServiceUnavailable(e.to_string()),
            SecurityError::ChunkReadTimeout { .. } => TeeError::Timeout,
            SecurityError::ProxyError(status) => {
                TeeError::BadGateway(format!("オフチェーンストレージがエラーを返しました: HTTP {status}"))
            }
            _ => TeeError::BadGateway(format!("signed_jsonの取得に失敗: {e}")),
        })?;

        // signed_jsonをパース
        let signed_json: SignedJson = serde_json::from_slice(&proxy_response.body)
            .map_err(|e| TeeError::BadRequest(format!("signed_jsonのパースに失敗: {e}")))?;

        // Step 2: tee_signatureを自身の公開鍵で検証
        // 仕様書 §6.4: 自身が生成したsigned_jsonであることの確認
        // TEE再起動（鍵ローテーション）後は旧signed_jsonが自動的に拒否される
        let sig_bytes = b64().decode(&signed_json.core.tee_signature)
            .map_err(|e| TeeError::BadRequest(format!("tee_signatureのBase64デコードに失敗: {e}")))?;
        let sig_arr: [u8; 64] = sig_bytes.try_into()
            .map_err(|_| TeeError::BadRequest("tee_signatureは64バイトである必要があります".into()))?;
        let ed_signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

        // 署名対象を再構築して検証
        let sign_target = serde_json::json!({
            "payload": signed_json.payload,
            "attributes": signed_json.attributes,
        });
        let sign_bytes = serde_json::to_vec(&sign_target)
            .map_err(|e| TeeError::Internal(format!("署名対象のシリアライズに失敗: {e}")))?;

        verifying_key
            .verify_strict(&sign_bytes, &ed_signature)
            .map_err(|_| TeeError::Forbidden(
                "tee_signatureの検証に失敗しました。TEEが再起動した可能性があります".into(),
            ))?;

        // Step 3: Bubblegum V2 cNFT発行トランザクション構築
        // creator_walletを取得（仕様書 §5.1 Step 9）
        let creator_wallet_str = signed_json
            .payload
            .get("creator_wallet")
            .and_then(|v| v.as_str())
            .ok_or(TeeError::BadRequest("signed_json.payload.creator_walletが見つかりません".into()))?;
        let creator_wallet = Pubkey::from_str(creator_wallet_str)
            .map_err(|e| TeeError::BadRequest(format!("creator_walletのBase58デコードに失敗: {e}")))?;

        // content_hashを取得
        let content_hash = signed_json
            .payload
            .get("content_hash")
            .and_then(|v| v.as_str())
            .ok_or(TeeError::BadRequest("signed_json.payload.content_hashが見つかりません".into()))?;

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

        solana_tx::apply_partial_signature(&mut tx, &tee_signing_pubkey, &tee_sig)
            .map_err(|e| TeeError::Internal(format!("TEE署名の適用に失敗: {e}")))?;

        // Step 5: 部分署名済みトランザクションを返却
        let tx_bytes = solana_tx::serialize_transaction(&tx)
            .map_err(|e| TeeError::Internal(format!("トランザクションのシリアライズに失敗: {e}")))?;

        partial_txs.push(b64().encode(&tx_bytes));
    }

    Ok(Json(SignResponse { partial_txs }))
}
