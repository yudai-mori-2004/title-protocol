//! # TEE エラー型
//!
//! 仕様書 §6.4
//!
//! 全エンドポイントで共通のエラー型。
//! `GatewayError`（`crates/gateway/src/error.rs`）と同パターン。

use axum::http::StatusCode;

/// TEEエラー型。
/// 仕様書 §6.4
#[derive(Debug, thiserror::Error)]
pub enum TeeError {
    /// 不正なリクエスト（パース失敗、Base64デコード失敗、不正入力）
    #[error("不正なリクエスト: {0}")]
    BadRequest(String),
    /// 内部エラー（鍵取得失敗、シリアライズ失敗）
    #[error("内部エラー: {0}")]
    Internal(String),
    /// サーバー状態が不正（inactive/active状態の不一致）
    #[error("サーバーが不正な状態です: {0}")]
    InvalidState(String),
    /// 二重呼び出し（/create-tree の2回目等）
    #[error("{0}")]
    Conflict(String),
    /// ペイロードサイズ超過
    #[error("ペイロードサイズが上限を超えています: {0}")]
    PayloadTooLarge(String),
    /// リクエストタイムアウト
    #[error("リクエスト処理がタイムアウトしました")]
    Timeout,
    /// 外部通信失敗（Proxy/Temporary Storage）
    #[error("外部通信に失敗: {0}")]
    BadGateway(String),
    /// 検証・処理失敗（C2PA検証、WASM実行）
    #[error("検証処理に失敗: {0}")]
    ProcessingFailed(String),
    /// 信頼されていないExtension
    #[error("{0}")]
    Forbidden(String),
    /// Gateway認証失敗
    #[error("Gateway認証に失敗: {0}")]
    Unauthorized(String),
    /// メモリ制限到達
    #[error("メモリ制限に達しました")]
    ServiceUnavailable(String),
}

impl axum::response::IntoResponse for TeeError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            TeeError::BadRequest(_) => StatusCode::BAD_REQUEST,
            TeeError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            TeeError::InvalidState(_) | TeeError::ServiceUnavailable(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            TeeError::Conflict(_) => StatusCode::CONFLICT,
            TeeError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            TeeError::Timeout => StatusCode::REQUEST_TIMEOUT,
            TeeError::BadGateway(_) => StatusCode::BAD_GATEWAY,
            TeeError::ProcessingFailed(_) => StatusCode::UNPROCESSABLE_ENTITY,
            TeeError::Forbidden(_) => StatusCode::FORBIDDEN,
            TeeError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
        };
        (status, self.to_string()).into_response()
    }
}
