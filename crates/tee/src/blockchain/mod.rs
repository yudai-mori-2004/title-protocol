//! # ブロックチェーン連携モジュール
//!
//! 仕様書 §5.1, §6.4
//!
//! Solana上のBubblegum V2 (cNFT) トランザクション構築を行う。

#[allow(deprecated)] // solana-sdk 2.x のsystem_instruction/system_program非推奨警告を抑制
pub mod solana_tx;
