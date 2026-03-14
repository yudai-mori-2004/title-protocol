// SPDX-License-Identifier: Apache-2.0

//! # POST /sign-and-mint
//!
//! 仕様書 §6.2
//!
//! sign + ブロードキャスト代行。
//! signed_json本体の保存代行にも対応（ノード運営者のオプション機能）。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use serde::Deserialize;
use title_types::*;

use crate::auth::{b64, relay_to_tee};
use crate::config::GatewayState;
use crate::error::GatewayError;

// ---------------------------------------------------------------------------
// Gateway固有のリクエスト型（signed_json本体対応）
// ---------------------------------------------------------------------------

/// /sign-and-mint リクエストの個別アイテム（Gateway固有）。
///
/// `signed_json_uri` と `signed_json` の2パターンに対応:
/// - `signed_json_uri`: クライアントが事前に保存済みのURI
/// - `signed_json`: Gatewayに保存を代行させる場合のJSON本体
#[derive(Debug, Deserialize)]
pub(crate) struct SignAndMintItem {
    /// オフチェーンストレージのURI（既にsigned_jsonが保存されている場合）
    #[serde(default)]
    pub signed_json_uri: String,
    /// signed_json本体（Gatewayに保存を代行させる場合）
    #[serde(default)]
    pub signed_json: Option<serde_json::Value>,
}

/// /sign-and-mint リクエスト（Gateway固有、signed_json本体対応）。
#[derive(Debug, Deserialize)]
pub(crate) struct SignAndMintInput {
    /// Base58エンコードされたBlockhash（空の場合はGatewayが自動取得）
    #[serde(default)]
    pub recent_blockhash: String,
    /// 署名リクエストの一覧
    pub requests: Vec<SignAndMintItem>,
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

/// POST /sign-and-mint — sign + ブロードキャスト代行。
/// 仕様書 §6.2
///
/// /signと同様にTEEから部分署名済みトランザクションを取得し、
/// GatewayのSolanaウォレットで最終署名を行い、Solanaにブロードキャストする。
/// クライアントはSolanaウォレットでの署名を省略でき、ガス代はGateway運営者が負担する。
///
/// `signed_json` 本体が渡された場合、Gatewayが保存を代行しURIに変換してからTEEに中継する。
/// この機能は `signed_json_storage` が設定されている場合のみ利用可能。
pub async fn handle_sign_and_mint(
    State(state): State<Arc<GatewayState>>,
    Json(input): Json<SignAndMintInput>,
) -> Result<Json<SignAndMintResponse>, GatewayError> {
    let solana_rpc_url = state
        .solana_rpc_url
        .as_ref()
        .ok_or_else(|| GatewayError::Internal("SOLANA_RPC_URLが設定されていません".to_string()))?;
    let gateway_keypair = state.solana_keypair.as_ref().ok_or_else(|| {
        GatewayError::Internal("GATEWAY_SOLANA_KEYPAIRが設定されていません".to_string())
    })?;

    // Step 0: signed_json本体 → 保存 → URI変換
    let mut sign_items = Vec::new();
    for item in &input.requests {
        let uri = match (&item.signed_json, item.signed_json_uri.is_empty()) {
            (Some(sj), _) => {
                // signed_json本体 → ストレージに保存してURIを取得
                let router = state.signed_json_storage.as_ref().ok_or_else(|| {
                    GatewayError::BadRequest(
                        "このノードはsigned_json保存代行に対応していません。\
                         signed_json_uriを指定してください"
                            .to_string(),
                    )
                })?;
                let key = format!("signed-json/{}.json", uuid::Uuid::new_v4());
                let data = serde_json::to_vec(sj).map_err(|e| {
                    GatewayError::BadRequest(format!("signed_jsonのシリアライズに失敗: {e}"))
                })?;
                router.store(sj, &key, &data).await?
            }
            (None, false) => item.signed_json_uri.clone(),
            (None, true) => {
                return Err(GatewayError::BadRequest(
                    "signed_json_uriまたはsigned_jsonのいずれかが必要です".to_string(),
                ));
            }
        };
        sign_items.push(SignRequestItem {
            signed_json_uri: uri,
        });
    }

    // sign-and-mint時はGatewayウォレットがfee payerとなる
    use solana_sdk::signer::Signer;
    let gateway_pubkey_str = gateway_keypair.pubkey().to_string();

    let mut body = SignRequest {
        recent_blockhash: input.recent_blockhash,
        requests: sign_items,
        fee_payer: Some(gateway_pubkey_str),
    };

    // Step 1: recent_blockhashが空の場合、Solana RPCから最新のblockhashを取得
    if body.recent_blockhash.is_empty() {
        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [{"commitment": "confirmed"}]
        });
        let rpc_response = state
            .http_client
            .post(solana_rpc_url)
            .json(&rpc_request)
            .send()
            .await
            .map_err(|e| GatewayError::Solana(format!("blockhash取得失敗: {e}")))?;
        let rpc_body: serde_json::Value = rpc_response
            .json()
            .await
            .map_err(|e| GatewayError::Solana(format!("blockhashレスポンスのパースに失敗: {e}")))?;
        let blockhash = rpc_body
            .pointer("/result/value/blockhash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                GatewayError::Solana("blockhashの取得に失敗しました".to_string())
            })?;
        body.recent_blockhash = blockhash.to_string();
    }

    // Step 2: TEEの/signに中継
    let body_value = serde_json::to_value(&body)
        .map_err(|e| GatewayError::Internal(format!("リクエストのシリアライズに失敗: {e}")))?;

    let result = relay_to_tee(&state, "/sign", body_value).await?;
    let sign_response: SignResponse = serde_json::from_value(result)
        .map_err(|e| GatewayError::TeeRelay(format!("SignResponseのパースに失敗: {e}")))?;

    // Step 3: 各partial_txにGatewayウォレットで署名+ブロードキャスト
    let mut tx_signatures = Vec::new();

    for partial_tx_b64 in &sign_response.partial_txs {
        let tx_bytes = b64().decode(partial_tx_b64).map_err(|e| {
            GatewayError::TeeRelay(format!("partial_txのBase64デコードに失敗: {e}"))
        })?;

        let mut tx: solana_sdk::transaction::Transaction =
            bincode::deserialize(&tx_bytes).map_err(|e| {
                GatewayError::TeeRelay(format!(
                    "トランザクションのデシリアライズに失敗: {e}"
                ))
            })?;

        // Gatewayウォレットで署名（未署名のスロットに署名）
        let gateway_pubkey = gateway_keypair.pubkey();

        // Gatewayの公開鍵に対応する署名スロットを特定
        // 署名者はaccount_keysの先頭num_required_signatures個に限定される
        let num_signers = tx.message.header.num_required_signatures as usize;
        let sig_index = tx
            .message
            .account_keys
            .iter()
            .take(num_signers)
            .position(|k| *k == gateway_pubkey)
            .ok_or_else(|| {
                GatewayError::Internal(
                    "Gatewayの公開鍵がトランザクションの署名者に含まれていません".to_string(),
                )
            })?;

        let message_bytes = tx.message.serialize();
        let sig = gateway_keypair.sign_message(&message_bytes);
        tx.signatures[sig_index] = sig;

        // Solana RPCにブロードキャスト
        let tx_serialized = bincode::serialize(&tx)
            .map_err(|e| GatewayError::Internal(format!("トランザクションのシリアライズに失敗: {e}")))?;
        let tx_b64 = b64().encode(&tx_serialized);

        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [tx_b64, {"encoding": "base64", "skipPreflight": true, "preflightCommitment": "confirmed"}]
        });

        let rpc_response = state
            .http_client
            .post(solana_rpc_url)
            .json(&rpc_request)
            .send()
            .await
            .map_err(|e| GatewayError::Solana(format!("RPC送信失敗: {e}")))?;

        let rpc_body: serde_json::Value = rpc_response
            .json()
            .await
            .map_err(|e| GatewayError::Solana(format!("RPCレスポンスのパースに失敗: {e}")))?;

        if let Some(error) = rpc_body.get("error") {
            return Err(GatewayError::Solana(format!(
                "トランザクションのブロードキャストに失敗: {error}"
            )));
        }

        let tx_sig = rpc_body
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                GatewayError::Solana("RPCレスポンスにresultがありません".to_string())
            })?;

        tx_signatures.push(tx_sig.to_string());
    }

    Ok(Json(SignAndMintResponse { tx_signatures }))
}
