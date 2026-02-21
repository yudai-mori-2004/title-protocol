//! # /create-tree エンドポイント
//!
//! 仕様書 §6.4 Step 2
//!
//! TEE起動直後にinactive状態で一度だけ公開される。
//! Bubblegum V2 CreateTreeConfig トランザクションを構築し、部分署名して返却する。
//! 呼び出し後、TEEはactive状態に遷移する。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::Engine;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use title_types::{CreateTreeRequest, CreateTreeResponse};

use crate::solana_tx;
use crate::{AppState, TeeState};

/// Base64エンジン（Standard）
fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// /create-tree エンドポイントハンドラ。
/// 仕様書 §6.4 Step 2, §6.5 Merkle Tree
///
/// このエンドポイントはTEEインスタンスの生存期間中に一度だけ呼び出し可能。
/// 二度目以降の呼び出しはエラーを返す。
pub async fn handle_create_tree(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<CreateTreeResponse>, (axum::http::StatusCode, String)> {
    // inactive状態チェック（二重呼び出し防止）
    {
        let current = state.state.read().await;
        if *current != TeeState::Inactive {
            return Err((
                axum::http::StatusCode::CONFLICT,
                "TEEは既にactive状態です。/create-treeは一度だけ呼び出し可能です".to_string(),
            ));
        }
    }

    // リクエストパース
    let request: CreateTreeRequest = serde_json::from_value(body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("CreateTreeRequestのパースに失敗: {e}"),
        )
    })?;

    // recent_blockhash（Base58デコード）
    let blockhash = solana_sdk::hash::Hash::from_str(&request.recent_blockhash).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("recent_blockhashのBase58デコードに失敗: {e}"),
        )
    })?;

    // Tree公開鍵
    let tree_pubkey_bytes: [u8; 32] = state.runtime.tree_pubkey().try_into().map_err(|_| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Tree公開鍵の取得に失敗".to_string(),
        )
    })?;
    let tree_pubkey = Pubkey::new_from_array(tree_pubkey_bytes);

    // TEE署名用公開鍵（payer兼tree_creator）
    // 仕様書 §6.4: payerをTEE内部walletにすることで、
    // Merkle Treeの作成・操作権限が完全にTEE内部に閉じる。
    let signing_pubkey_bytes: [u8; 32] =
        state.runtime.signing_pubkey().try_into().map_err(|_| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "署名用公開鍵の取得に失敗".to_string(),
            )
        })?;
    let tee_signing_pubkey = Pubkey::new_from_array(signing_pubkey_bytes);

    // Bubblegum V2 CreateTreeConfig トランザクション構築（仕様書 §6.4 Step 2）
    // payer = TEE signing_pubkey（TEE内部walletが支払う）
    let mut tx = solana_tx::build_create_tree_tx(
        &tee_signing_pubkey,
        &tree_pubkey,
        &tee_signing_pubkey,
        request.max_depth,
        request.max_buffer_size,
        &blockhash,
    );

    // TEEが全署名を行う（payer=signing_key なので signing_key + tree_key の2署名）
    let message_bytes = tx.message.serialize();

    let tree_sig = state.runtime.tree_sign(&message_bytes);
    solana_tx::apply_partial_signature(&mut tx, &tree_pubkey, &tree_sig).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Tree署名の適用に失敗: {e}"),
        )
    })?;

    let signing_sig = state.runtime.sign(&message_bytes);
    solana_tx::apply_partial_signature(&mut tx, &tee_signing_pubkey, &signing_sig).map_err(
        |e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("TEE署名の適用に失敗: {e}"),
            )
        },
    )?;

    // トランザクションシリアライズ
    let tx_bytes = solana_tx::serialize_transaction(&tx).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("トランザクションのシリアライズに失敗: {e}"),
        )
    })?;

    // Tree addressを保存
    {
        let mut tree_addr = state.tree_address.write().await;
        *tree_addr = Some(tree_pubkey_bytes);
    }

    // 状態遷移: inactive → active (仕様書 §6.4 Step 3)
    {
        let mut current = state.state.write().await;
        *current = TeeState::Active;
    }

    tracing::info!(
        tree_address = %tree_pubkey,
        "Merkle Tree作成トランザクションを構築しました。TEEはactive状態に遷移しました"
    );

    let response = CreateTreeResponse {
        signed_tx: b64().encode(&tx_bytes),
        tree_address: tree_pubkey.to_string(),
        signing_pubkey: tee_signing_pubkey.to_string(),
        encryption_pubkey: b64().encode(state.runtime.encryption_pubkey()),
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::mock::MockRuntime;
    use crate::runtime::TeeRuntime;
    use solana_sdk::transaction::Transaction;
    use tokio::sync::{RwLock, Semaphore};

    fn make_test_state() -> Arc<AppState> {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();
        rt.generate_tree_keypair();

        Arc::new(AppState {
            runtime: Box::new(rt),
            state: RwLock::new(TeeState::Inactive),
            proxy_addr: "127.0.0.1:0".to_string(),
            tree_address: RwLock::new(None),
            collection_mint: None,
            gateway_pubkey: None,
            wasm_loader: None,
            memory_semaphore: Arc::new(Semaphore::new(1024 * 1024 * 1024)),
            trusted_extension_ids: None,
        })
    }

    /// /create-tree が正常に動作し、inactive → active に遷移することを確認
    /// payer = TEE signing_pubkey で完全署名済みTXが返る
    #[tokio::test]
    async fn test_create_tree_success() {
        let state = make_test_state();

        let body = serde_json::json!({
            "max_depth": 20,
            "max_buffer_size": 64,
            "recent_blockhash": "11111111111111111111111111111111",
        });

        let result = handle_create_tree(State(state.clone()), Json(body)).await;
        assert!(result.is_ok(), "handle_create_tree failed: {:?}", result.err());

        let response = result.unwrap().0;

        // signed_txがBase64でデコード可能
        let tx_bytes = b64().decode(&response.signed_tx).unwrap();
        assert!(!tx_bytes.is_empty());

        // トランザクションがデシリアライズ可能
        let tx: Transaction = bincode::deserialize(&tx_bytes).unwrap();
        // payer = tree_creator なので署名者は2（signing_key + tree_key）
        assert_eq!(tx.message.header.num_required_signatures, 2);
        // 3つの命令（compute_budget + create_account + create_tree_config_v2）
        assert_eq!(tx.message.instructions.len(), 3);

        // tree_addressがBase58
        assert!(!response.tree_address.is_empty());

        // signing_pubkeyがBase58
        assert!(!response.signing_pubkey.is_empty());

        // encryption_pubkeyがBase64
        let enc_pk = b64().decode(&response.encryption_pubkey).unwrap();
        assert_eq!(enc_pk.len(), 32);

        // TEE状態がactiveに遷移
        let current = state.state.read().await;
        assert_eq!(*current, TeeState::Active);

        // tree_addressが設定されている
        let tree_addr = state.tree_address.read().await;
        assert!(tree_addr.is_some());
    }

    /// active状態での二度目の/create-tree呼び出しが409を返すことを確認
    #[tokio::test]
    async fn test_create_tree_already_active() {
        let state = make_test_state();

        // 先にactive状態にする
        {
            let mut current = state.state.write().await;
            *current = TeeState::Active;
        }

        let body = serde_json::json!({
            "max_depth": 20,
            "max_buffer_size": 64,
            "recent_blockhash": "11111111111111111111111111111111",
        });

        let result = handle_create_tree(State(state), Json(body)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::CONFLICT);
    }
}
