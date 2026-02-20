//! # POST /sign-and-mint
//!
//! 仕様書 §6.2
//!
//! sign + ブロードキャスト代行。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use title_types::*;

use crate::auth::{b64, relay_to_tee};
use crate::config::GatewayState;
use crate::error::GatewayError;

/// POST /sign-and-mint — sign + ブロードキャスト代行。
/// 仕様書 §6.2
///
/// /signと同様にTEEから部分署名済みトランザクションを取得し、
/// GatewayのSolanaウォレットで最終署名を行い、Solanaにブロードキャストする。
/// クライアントはSolanaウォレットでの署名を省略でき、ガス代はGateway運営者が負担する。
pub async fn handle_sign_and_mint(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<SignRequest>,
) -> Result<Json<SignAndMintResponse>, GatewayError> {
    let solana_rpc_url = state
        .solana_rpc_url
        .as_ref()
        .ok_or_else(|| GatewayError::Internal("SOLANA_RPC_URLが設定されていません".to_string()))?;
    let gateway_keypair = state.solana_keypair.as_ref().ok_or_else(|| {
        GatewayError::Internal("GATEWAY_SOLANA_KEYPAIRが設定されていません".to_string())
    })?;

    // Step 1: TEEの/signに中継
    let body_value = serde_json::to_value(&body)
        .map_err(|e| GatewayError::Internal(format!("リクエストのシリアライズに失敗: {e}")))?;

    let result = relay_to_tee(&state, "/sign", body_value).await?;
    let sign_response: SignResponse = serde_json::from_value(result)
        .map_err(|e| GatewayError::TeeRelay(format!("SignResponseのパースに失敗: {e}")))?;

    // Step 2: 各partial_txにGatewayウォレットで署名+ブロードキャスト
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
        use solana_sdk::signer::Signer;
        let gateway_pubkey = gateway_keypair.pubkey();

        // Gatewayの公開鍵に対応する署名スロットを特定
        let sig_index = tx
            .message
            .account_keys
            .iter()
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
            "params": [tx_b64, {"encoding": "base64"}]
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
