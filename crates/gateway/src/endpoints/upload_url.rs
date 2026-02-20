//! # POST /upload-url
//!
//! 仕様書 §6.2
//!
//! Temporary Storageへの署名付きURL発行。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::Json;
use title_types::*;

use crate::config::GatewayState;
use crate::error::GatewayError;

/// POST /upload-url — 署名付きURL発行。
/// 仕様書 §6.2
///
/// Temporary Storageへのアップロード用署名付きURLを発行する。
/// content-length-range条件によるEDoS攻撃対策を含む。
pub async fn handle_upload_url(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<UploadUrlRequest>,
) -> Result<Json<UploadUrlResponse>, GatewayError> {
    // EDoS対策: コンテンツサイズの上限チェック (仕様書 §6.2)
    if body.content_size > state.max_upload_size {
        return Err(GatewayError::BadRequest(format!(
            "コンテンツサイズが上限を超えています: {} bytes (上限: {} bytes)",
            body.content_size, state.max_upload_size
        )));
    }

    if body.content_size == 0 {
        return Err(GatewayError::BadRequest(
            "コンテンツサイズは1以上である必要があります".to_string(),
        ));
    }

    // ユニークなオブジェクトキーを生成
    let object_key = format!("uploads/{}", uuid::Uuid::new_v4());

    // TempStorageトレイト経由で署名付きURLを生成
    let urls = state
        .temp_storage
        .generate_presigned_urls(&object_key, state.presign_expiry_secs)
        .await?;

    // URL有効期限のUNIXタイムスタンプ
    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| GatewayError::Internal(format!("時刻取得失敗: {e}")))?
        .as_secs()
        + state.presign_expiry_secs as u64;

    Ok(Json(UploadUrlResponse {
        upload_url: urls.upload_url,
        download_url: urls.download_url,
        expires_at,
    }))
}
