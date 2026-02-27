// SPDX-License-Identifier: Apache-2.0

//! # Gateway エラー型
//!
//! 仕様書 §6.2

use axum::http::StatusCode;

/// Gatewayエラー型。
/// 仕様書 §6.2
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// TEEへのリクエスト中継に失敗
    #[error("TEEへのリクエスト中継に失敗: {0}")]
    TeeRelay(String),
    /// ストレージ操作に失敗
    #[error("ストレージ操作に失敗: {0}")]
    Storage(String),
    /// Solana RPC エラー
    #[error("Solana RPC エラー: {0}")]
    Solana(String),
    /// 内部エラー
    #[error("内部エラー: {0}")]
    Internal(String),
    /// 不正なリクエスト
    #[error("不正なリクエスト: {0}")]
    BadRequest(String),
}

impl axum::response::IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            GatewayError::TeeRelay(_) => StatusCode::BAD_GATEWAY,
            GatewayError::Storage(_) | GatewayError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            GatewayError::Solana(_) => StatusCode::BAD_GATEWAY,
            GatewayError::BadRequest(_) => StatusCode::BAD_REQUEST,
        };
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    /// 全GatewayErrorバリアントが正しいHTTPステータスコードにマッピングされることを確認
    #[test]
    fn test_error_status_codes() {
        let cases: Vec<(GatewayError, StatusCode)> = vec![
            (GatewayError::TeeRelay("t".into()), StatusCode::BAD_GATEWAY),
            (
                GatewayError::Storage("t".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                GatewayError::Solana("t".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                GatewayError::Internal("t".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                GatewayError::BadRequest("t".into()),
                StatusCode::BAD_REQUEST,
            ),
        ];

        for (error, expected_status) in cases {
            let response = error.into_response();
            assert_eq!(
                response.status(),
                expected_status,
                "ステータスコードが一致しません"
            );
        }
    }
}
