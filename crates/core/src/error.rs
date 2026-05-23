// SPDX-License-Identifier: Apache-2.0

//! # コアエラー型
//!
//! title-core クレートのトップレベルエラー型。
//! processor個別のエラーは `processor::ProcessorError` で扱う。

/// title-core のエラー型。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// リクエストの形式が不正。
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// 指定されたprocessorが見つからない。
    #[error("Unknown processor: {id}")]
    UnknownProcessor {
        /// 見つからなかったprocessor ID。
        id: String,
    },

    /// JSONシリアライズ/デシリアライズエラー。
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// 内部エラー。
    #[error("Internal error: {0}")]
    Internal(String),
}
