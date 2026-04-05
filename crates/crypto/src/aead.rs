// SPDX-License-Identifier: Apache-2.0

//! # 認証付き暗号（AEAD）トレイト
//!
//! 仕様書 §6.4
//!
//! 根拠: RustCrypto `aead` crate, RFC 5116 (AEAD interface), HPKE (RFC 9180 §5.2)
//!
//! AAD（Additional Authenticated Data）パラメータは必須。
//! 使用しない場合は `&[]` を明示的に渡す。

use crate::algorithm::AeadAlgorithm;
use crate::error::CryptoError;

/// 認証付き暗号化/復号。対称鍵を内部に保持する。
///
/// nonce長はアルゴリズムに依存する（AES-GCM: 12B, XChaCha: 24B）。
/// AADはreplay/swapping攻撃防止のためのコンテキストバインディングに使用。
pub trait Aead: Send + Sync {
    /// このAEADが使用するアルゴリズム。
    fn algorithm(&self) -> AeadAlgorithm;

    /// nonce長（バイト）。
    fn nonce_size(&self) -> usize;

    /// 暗号化。ciphertext + authタグを返す。
    fn encrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;

    /// 復号。
    fn decrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;
}
