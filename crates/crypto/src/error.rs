// SPDX-License-Identifier: Apache-2.0

//! # 暗号処理エラー型
//!
//! 仕様書 §6.4

/// 暗号処理のエラー型
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// ECDH鍵交換エラー
    #[error("ECDH鍵交換に失敗しました")]
    EcdhError,
    /// HKDF鍵導出エラー
    #[error("HKDF鍵導出に失敗しました: {0}")]
    HkdfError(String),
    /// 暗号化エラー
    #[error("暗号化に失敗しました")]
    EncryptError,
    /// 復号エラー
    #[error("復号に失敗しました")]
    DecryptError,
    /// 署名検証エラー
    #[error("署名検証に失敗しました")]
    SignatureVerifyError,
    /// 鍵長不正
    #[error("鍵長が不正です: expected {expected}, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },
    /// 鍵フォーマット不正
    #[error("鍵フォーマットが不正です: {0}")]
    InvalidKeyFormat(String),
    /// 未対応アルゴリズム
    #[error("未対応のアルゴリズムです: {0}")]
    UnsupportedAlgorithm(String),
    /// ワイヤーフォーマット不正
    #[error("ワイヤーフォーマットが不正です: {0}")]
    InvalidWireFormat(String),
    /// 未対応暗号スイート
    #[error("未対応の暗号スイートです: 0x{0:02x}")]
    UnsupportedSuite(u8),
    /// Nonce枯渇
    #[error("nonce空間が枯渇しました")]
    NonceExhausted,
}
