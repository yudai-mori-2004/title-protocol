// SPDX-License-Identifier: Apache-2.0

//! 最小限のSolana JSON-RPC クライアント（reqwestベース）。

use base64::Engine;
use serde::{Deserialize, Serialize};
use solana_sdk::{hash::Hash, pubkey::Pubkey};

use crate::error::CliError;

/// Base64エンコーディングエンジン。
pub(crate) fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Solana JSON-RPC クライアント。
pub struct SolanaRpc {
    client: reqwest::Client,
    url: String,
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: serde_json::Value,
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Deserialize)]
struct BlockhashResult {
    value: BlockhashValue,
}

#[derive(Deserialize)]
struct BlockhashValue {
    blockhash: String,
}

#[derive(Deserialize)]
struct BalanceResult {
    value: u64,
}

#[derive(Deserialize)]
struct AccountInfoResult {
    value: Option<AccountInfoValue>,
}

#[derive(Deserialize)]
struct AccountInfoValue {
    data: (String, String), // (base64_data, encoding)
}

impl SolanaRpc {
    pub fn new(url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: url.to_string(),
        }
    }

    /// JSON-RPCリクエストを送信する。
    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, CliError> {
        let req = RpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };
        let resp: RpcResponse<T> = self
            .client
            .post(&self.url)
            .json(&req)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.error {
            return Err(CliError::Rpc(err.message));
        }
        resp.result
            .ok_or_else(|| CliError::Rpc("レスポンスにresultがありません".into()))
    }

    /// 最新のBlockhashを取得する。
    pub async fn get_latest_blockhash(&self) -> Result<Hash, CliError> {
        let result: BlockhashResult = self
            .call(
                "getLatestBlockhash",
                serde_json::json!([{"commitment": "confirmed"}]),
            )
            .await?;
        result
            .value
            .blockhash
            .parse()
            .map_err(|e| CliError::Rpc(format!("blockhashのパースに失敗: {e}")))
    }

    /// アカウントの残高（lamports）を取得する。
    pub async fn get_balance(&self, pubkey: &Pubkey) -> Result<u64, CliError> {
        let result: BalanceResult = self
            .call(
                "getBalance",
                serde_json::json!([pubkey.to_string(), {"commitment": "confirmed"}]),
            )
            .await?;
        Ok(result.value)
    }

    /// アカウントデータを取得する（存在しなければNone）。
    pub async fn get_account_data(&self, pubkey: &Pubkey) -> Result<Option<Vec<u8>>, CliError> {
        let result: AccountInfoResult = self
            .call(
                "getAccountInfo",
                serde_json::json!([pubkey.to_string(), {"encoding": "base64", "commitment": "confirmed"}]),
            )
            .await?;

        match result.value {
            Some(info) => {
                let data = b64()
                    .decode(&info.data.0)
                    .map_err(|e| CliError::Rpc(format!("アカウントデータのデコードに失敗: {e}")))?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// 署名済みトランザクションを送信する（base64エンコード）。
    pub async fn send_transaction(&self, tx_bytes: &[u8]) -> Result<String, CliError> {
        let encoded = b64().encode(tx_bytes);
        let sig: String = self
            .call(
                "sendTransaction",
                serde_json::json!([encoded, {"encoding": "base64", "skipPreflight": false}]),
            )
            .await?;
        Ok(sig)
    }

    /// トランザクションの確認を待つ（簡易ポーリング）。
    pub async fn confirm_transaction(&self, signature: &str) -> Result<(), CliError> {
        for _ in 0..30 {
            let result: serde_json::Value = self
                .call(
                    "getSignatureStatuses",
                    serde_json::json!([[signature]]),
                )
                .await?;

            if let Some(statuses) = result.get("value").and_then(|v| v.as_array()) {
                if let Some(status) = statuses.first().and_then(|s| s.as_object()) {
                    if status.get("err").is_some_and(|e| !e.is_null()) {
                        let err_str = status.get("err").unwrap().to_string();
                        return Err(CliError::Transaction(err_str));
                    }
                    if status
                        .get("confirmationStatus")
                        .and_then(|s| s.as_str())
                        .is_some_and(|s| s == "confirmed" || s == "finalized")
                    {
                        return Ok(());
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(CliError::Transaction(
            "トランザクション確認がタイムアウトしました".into(),
        ))
    }

    /// Airdropをリクエストする（devnetのみ）。
    pub async fn request_airdrop(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<String, CliError> {
        let sig: String = self
            .call(
                "requestAirdrop",
                serde_json::json!([pubkey.to_string(), lamports]),
            )
            .await?;
        Ok(sig)
    }

    /// トランザクション送信+確認を一括実行する。
    pub async fn send_and_confirm(&self, tx_bytes: &[u8]) -> Result<String, CliError> {
        let sig = self.send_transaction(tx_bytes).await?;
        self.confirm_transaction(&sig).await?;
        Ok(sig)
    }
}
