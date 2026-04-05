// SPDX-License-Identifier: Apache-2.0

//! /sign ハンドラ実装

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use solana_sdk::message::AddressLookupTableAccount;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use title_types::{SignRequest, SignResponse, SignedJson};

use crate::config::{TeeAppState, TeeState};
use crate::error::TeeError;
use crate::infra::security::{self, SecurityError};
use crate::blockchain::solana_tx;
use crate::endpoints::b64;

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
        crate::infra::gateway_auth::verify_gateway_auth(state.gateway_pubkey.as_deref(), &body)
            .map_err(|(_, msg)| TeeError::Unauthorized(msg))?;

    let request: SignRequest = serde_json::from_value(inner_body)
        .map_err(|e| TeeError::BadRequest(format!("SignRequestのパースに失敗: {e}")))?;

    // fee_payer（sign-and-mint時にGatewayウォレットをfee payerとして使用）
    let fee_payer_pubkey = match &request.fee_payer {
        Some(fp) => Some(Pubkey::from_str(fp)
            .map_err(|e| TeeError::BadRequest(format!("fee_payerのBase58デコードに失敗: {e}")))?),
        None => None,
    };

    // resource_limitsの適用（§6.4）
    let limits = security::resolve_limits(resource_limits.as_ref());
    let chunk_timeout = Duration::from_secs(limits.chunk_read_timeout_sec);

    // recent_blockhash（Base58デコード）
    let blockhash = solana_sdk::hash::Hash::from_str(&request.recent_blockhash)
        .map_err(|e| TeeError::BadRequest(format!("recent_blockhashのBase58デコードに失敗: {e}")))?;

    // Solana TX署名用公開鍵
    let tee_pubkey_bytes: [u8; 32] = state.runtime.solana_signer().public_key_bytes().try_into()
        .map_err(|_| TeeError::Internal("署名用公開鍵の取得に失敗".into()))?;
    let tee_signing_pubkey = Pubkey::new_from_array(tee_pubkey_bytes);

    // Protocol署名検証用Verifier（アルゴリズムはsigned_jsonのtee_signature_algorithmから決定）
    let protocol_pubkey = state.runtime.protocol_signer().public_key_bytes();
    let protocol_verifier = title_crypto::create_verifier(
        state.runtime.protocol_signing_algorithm(),
        &protocol_pubkey,
    ).map_err(|e| TeeError::Internal(format!("検証用公開鍵の構築に失敗: {e}")))?;

    // 動的グローバルタイムアウト適用（仕様書 §6.4）
    // ContentSize = items数 × MAX_SIGNED_JSON_SIZE（最悪ケース見積もり）
    let total_content_estimate = request.requests.len() as u64 * security::MAX_SIGNED_JSON_SIZE;
    let global_timeout = security::compute_dynamic_timeout(&limits, total_content_estimate);

    let partial_txs = tokio::time::timeout(global_timeout, async {

    // 全 item を並列にダウンロード + 検証 + instruction 構築
    let futures: Vec<_> = request.requests.iter().map(|item| {
        let state = &state;
        let protocol_verifier = &protocol_verifier;
        let tee_signing_pubkey = &tee_signing_pubkey;
        let fee_payer_pubkey = &fee_payer_pubkey;
        let limits = &limits;
        async move {
            // Step 1: signed_json_uriからJSONをフェッチ（Verify on Sign）
            // 仕様書 §6.4 /signフェーズでの防御
            let download_timeout =
                security::compute_dynamic_timeout(limits, security::MAX_SIGNED_JSON_SIZE);
            let (proxy_response, _sign_ticket) = tokio::time::timeout(
                download_timeout,
                security::proxy_get_secured(
                    &state.proxy_addr,
                    &item.signed_json_uri,
                    security::MAX_SIGNED_JSON_SIZE,
                    chunk_timeout,
                    &state.resource_pool,
                ),
            )
            .await
            .map_err(|_| TeeError::Timeout)?
            .map_err(|e| match &e {
                SecurityError::PayloadTooLarge { .. } => TeeError::PayloadTooLarge(format!("signed_jsonのサイズが上限を超えています: {e}")),
                SecurityError::MemoryLimitExceeded => TeeError::ServiceUnavailable(e.to_string()),
                SecurityError::ChunkReadTimeout { .. } => TeeError::Timeout,
                SecurityError::ProxyError(status) => {
                    TeeError::BadGateway(format!("オフチェーンストレージがエラーを返しました: HTTP {status}"))
                }
                _ => TeeError::BadGateway(format!("signed_jsonの取得に失敗: {e}")),
            })?;

            let signed_json: SignedJson = serde_json::from_slice(&proxy_response.body)
                .map_err(|e| TeeError::BadRequest(format!("signed_jsonのパースに失敗: {e}")))?;

            // protocolに応じてTree/Collectionを選択（仕様書 §6.5）
            let is_extension = signed_json.core.protocol == "Title-Extension-v1";
            let tree_address_bytes = if is_extension {
                let addr = state.ext_tree_address.read().await;
                addr.ok_or(TeeError::Internal(
                    "Extension Merkle Treeが未作成です。先に/create-treeを呼び出してください".into(),
                ))?
            } else {
                let addr = state.core_tree_address.read().await;
                addr.ok_or(TeeError::Internal(
                    "Core Merkle Treeが未作成です。先に/create-treeを呼び出してください".into(),
                ))?
            };
            let tree_pubkey = Pubkey::new_from_array(tree_address_bytes);
            let collection_mint = if is_extension {
                state.ext_collection_mint.as_ref()
            } else {
                state.core_collection_mint.as_ref()
            };

            // Step 2: tee_signatureを自身の公開鍵で検証
            // 仕様書 §6.4: 自身が生成したsigned_jsonであることの確認
            let sig_bytes = b64().decode(&signed_json.core.tee_signature)
                .map_err(|e| TeeError::BadRequest(format!("tee_signatureのBase64デコードに失敗: {e}")))?;

            let sign_target = serde_json::json!({
                "payload": signed_json.payload,
                "attributes": signed_json.attributes,
            });
            let sign_bytes = serde_json_canonicalizer::to_vec(&sign_target)
                .map_err(|e| TeeError::Internal(format!("署名対象のJCS正規化に失敗: {e}")))?;

            // ドメインタグ付きで検証（core.rsでの署名時と同一のタグ）
            let tagged = title_crypto::domain_tagged("title-protocol-v1", &sign_bytes);
            protocol_verifier
                .verify(&tagged, &sig_bytes)
                .map_err(|_| TeeError::Forbidden(
                    "tee_signatureの検証に失敗しました。TEEが再起動した可能性があります".into(),
                ))?;

            // Step 3: Bubblegum V2 cNFT発行トランザクション構築
            let creator_wallet_str = signed_json
                .payload
                .get("creator_wallet")
                .and_then(|v| v.as_str())
                .ok_or(TeeError::BadRequest("signed_json.payload.creator_walletが見つかりません".into()))?;
            let creator_wallet = Pubkey::from_str(creator_wallet_str)
                .map_err(|e| TeeError::BadRequest(format!("creator_walletのBase58デコードに失敗: {e}")))?;

            let content_hash = signed_json
                .payload
                .get("content_hash")
                .and_then(|v| v.as_str())
                .ok_or(TeeError::BadRequest("signed_json.payload.content_hashが見つかりません".into()))?;

            let payer = fee_payer_pubkey.as_ref().unwrap_or(&creator_wallet);
            let ix = solana_tx::build_mint_v2_ix(
                &tree_pubkey,
                tee_signing_pubkey,
                &creator_wallet,
                content_hash,
                &item.signed_json_uri,
                collection_mint,
                payer,
            );
            Ok::<_, TeeError>((ix, creator_wallet))
        }
    }).collect();

    let results = futures::future::join_all(futures).await;
    let mut mint_instructions = Vec::new();
    let mut creator_pubkey: Option<Pubkey> = None;
    for result in results {
        let (ix, creator_wallet) = result?;
        creator_pubkey.get_or_insert(creator_wallet);
        mint_instructions.push(ix);
    }

    // ALT アカウント構築
    let alt_key = {
        let addr = state.alt_address.read().await;
        addr.ok_or(TeeError::InvalidState(
            "ALTが未設定です。先にALTを作成して /set-alt を呼び出してください".into(),
        ))?
    };
    let alt_addresses = state.alt_addresses.read().await.clone();
    let alt_account = AddressLookupTableAccount {
        key: alt_key,
        addresses: alt_addresses,
    };

    // ビンパッキング: VersionedTransaction (v0) + ALT で圧縮
    let tx_payer = fee_payer_pubkey.as_ref()
        .or(creator_pubkey.as_ref())
        .unwrap_or(&tee_signing_pubkey);
    let packed_txs = solana_tx::pack_mint_txs(mint_instructions, tx_payer, &blockhash, &alt_account);

    // 各TXにTEE部分署名を適用
    let mut partial_txs = Vec::new();
    for mut tx in packed_txs {
        let message_bytes = tx.message.serialize();
        let tee_sig = state.runtime.solana_signer().sign(&message_bytes)
            .map_err(|e| TeeError::Internal(format!("Solana TX署名に失敗: {e}")))?;

        solana_tx::apply_partial_signature(&mut tx, &tee_signing_pubkey, &tee_sig)
            .map_err(|e| TeeError::Internal(format!("TEE署名の適用に失敗: {e}")))?;

        let tx_bytes = solana_tx::serialize_transaction(&tx)
            .map_err(|e| TeeError::Internal(format!("トランザクションのシリアライズに失敗: {e}")))?;

        partial_txs.push(b64().encode(&tx_bytes));
    }

    Ok::<Vec<String>, TeeError>(partial_txs)
    })
    .await
    .map_err(|_| TeeError::Timeout)??;

    Ok(Json(SignResponse { partial_txs }))
}
