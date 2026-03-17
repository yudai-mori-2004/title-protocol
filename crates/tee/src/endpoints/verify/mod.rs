// SPDX-License-Identifier: Apache-2.0

//! # /verify エンドポイント
//!
//! 仕様書 §6.4 /verifyフェーズの内部処理
//!
//! ## モジュール構成
//! - `handler`: メインハンドラ（リクエスト受付・暗号化・復号）
//! - `core`: Core処理（C2PA検証 + 来歴グラフ構築）
//! - `extension`: Extension処理（WASM実行）

mod handler;
mod core;
mod extension;

pub use handler::handle_verify;

/// コンテンツのMIMEタイプをマジックバイトから検出する。
/// `infer` クレートに委譲し、対応形式の追従をライブラリに任せる。
/// 仕様書 §2.1
pub(crate) fn detect_mime_type(data: &[u8]) -> String {
    infer::get(data)
        .map(|t| t.mime_type().to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

/// content_hashを「0x」プレフィックス付きhex文字列に変換する。
/// 仕様書 §2.1
pub(crate) fn format_content_hash(hash: &[u8; 32]) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("0x{hex}")
}

/// Core プロセッサID。
pub(crate) const CORE_PROCESSOR_ID: &str = "core-c2pa";

#[cfg(test)]
mod tests;
