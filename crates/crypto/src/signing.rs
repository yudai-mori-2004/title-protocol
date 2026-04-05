// SPDX-License-Identifier: Apache-2.0

//! # 署名トレイト
//!
//! 仕様書 §5.1, §6.4
//!
//! 根拠: RustCrypto `signature` crate (`Signer`/`Verifier`)

use crate::algorithm::SigningAlgorithm;
use crate::error::CryptoError;

/// メッセージに署名する。秘密鍵を内部に保持し、外部に露出しない。
///
/// # 契約
/// - `sign` は `Result` を返す。HSM・リモート署名器でのI/Oエラーに対応。
///   （RustCrypto `signature::Signer::try_sign` と同等）
/// - cross-protocol攻撃を防ぐため、呼び出し側はドメインタグ付きメッセージを渡すべき。
pub trait Signer: Send + Sync {
    /// このSignerが使用するアルゴリズム。
    fn algorithm(&self) -> SigningAlgorithm;

    /// 公開鍵バイト列（アルゴリズム固有のエンコーディング）。
    fn public_key_bytes(&self) -> Vec<u8>;

    /// メッセージに署名し、署名バイト列を返す。
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, CryptoError>;
}

/// 署名を検証する。
pub trait Verifier: Send + Sync {
    /// このVerifierが使用するアルゴリズム。
    fn algorithm(&self) -> SigningAlgorithm;

    /// 公開鍵バイト列。
    fn public_key_bytes(&self) -> Vec<u8>;

    /// メッセージと署名を検証する。
    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), CryptoError>;
}
