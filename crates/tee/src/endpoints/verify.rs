//! # /verify エンドポイント
//!
//! 仕様書 §6.4 /verifyフェーズの内部処理
//!
//! ## 処理フロー
//! 1. Gateway署名を検証（Global Configのgateway_pubkeyを使用）
//! 2. resource_limitsが含まれていれば適用、なければデフォルト値を使用
//! 3. download_urlからTemporary Storage上の暗号化ペイロードを取得
//! 4. ペイロードを復号（ハイブリッド暗号化の逆操作）
//! 5. processor_idsに基づき、Core（C2PA検証＋来歴グラフ構築）およびExtension（WASM実行）を処理
//! 6. 検証結果をJSON形式でまとめ、TEE秘密鍵で署名（tee_signature）
//! 7. signed_jsonを共通鍵と新しいnonceでAES-GCM暗号化し返却

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::{AppState, TeeState};

/// /verify エンドポイントハンドラ。
/// 仕様書 §6.4
pub async fn handle_verify(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    // active状態チェック
    {
        let current = state.state.read().await;
        if *current != TeeState::Active {
            return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    // TODO: 1. Gateway署名の検証
    // TODO: 2. resource_limitsの適用
    // TODO: 3. download_urlから暗号化ペイロードをfetch
    // TODO: 4. ペイロード復号（ECDH → HKDF → AES-GCM）
    // TODO: 5. processor_idsに基づくCore/Extension実行
    //   - Core: C2PA検証 + 来歴グラフ構築
    //   - Extension: WASMモジュール実行
    // TODO: 6. signed_json生成 + TEE署名
    // TODO: 7. レスポンス暗号化（同一symmetric_key、新しいnonce）

    let _ = body;
    todo!("/verify エンドポイントの実装")
}
