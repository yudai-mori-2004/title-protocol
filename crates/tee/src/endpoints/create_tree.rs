//! # /create-tree エンドポイント
//!
//! 仕様書 §6.4 Step 2
//!
//! TEE起動直後にinactive状態で一度だけ公開される。
//! Merkle Tree作成トランザクションを構築し、部分署名して返却する。
//! 呼び出し後、TEEはactive状態に遷移する。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::{AppState, TeeState};

/// /create-tree エンドポイントハンドラ。
/// 仕様書 §6.4 Step 2
///
/// このエンドポイントはTEEインスタンスの生存期間中に一度だけ呼び出し可能。
/// 二度目以降の呼び出しはエラーを返す。
pub async fn handle_create_tree(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    // inactive状態チェック（二重呼び出し防止）
    {
        let current = state.state.read().await;
        if *current != TeeState::Inactive {
            return Err(axum::http::StatusCode::CONFLICT);
        }
    }

    // TODO: 1. リクエストからmax_depth, max_buffer_size, recent_blockhashを取得
    // TODO: 2. create_treeトランザクションを構築
    // TODO: 3. 署名用キーペアとTree用キーペアで部分署名
    // TODO: 4. レスポンス返却

    // 状態遷移: inactive → active (仕様書 §6.4 Step 3)
    {
        let mut current = state.state.write().await;
        *current = TeeState::Active;
    }

    let _ = body;
    todo!("/create-tree エンドポイントの実装")
}
