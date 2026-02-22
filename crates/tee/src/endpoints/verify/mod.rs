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

/// Base64エンジン（Standard）
pub(crate) fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// コンテンツのMIMEタイプをマジックバイトから検出する。
/// 仕様書 §2.1
pub(crate) fn detect_mime_type(data: &[u8]) -> &str {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.len() >= 12 && data[8..12] == *b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
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
