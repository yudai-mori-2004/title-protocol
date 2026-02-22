//! # /sign エンドポイント
//!
//! 仕様書 §6.4 /signフェーズの内部処理
//!
//! ## 処理フロー
//! 1. signed_json_uriからJSONをフェッチ（サイズ制限: 1MB）
//! 2. JSON内のtee_signatureを自身の公開鍵で検証
//! 3. payload.creator_walletを宛先としてBubblegum V2 cNFT発行トランザクションを構築
//! 4. TEEの秘密鍵で部分署名
//!
//! ## 防御策（Verify on Sign）
//! - JSONフェッチ時のサイズ制限（1MB上限）
//! - tee_signature検証によるTEE再起動時の自動拒否

mod handler;

#[cfg(test)]
mod tests;

pub use handler::handle_sign;
