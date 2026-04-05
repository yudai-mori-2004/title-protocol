// SPDX-License-Identifier: Apache-2.0

//! # 鍵カプセル化メカニズム（KEM）トレイト
//!
//! 仕様書 §6.4
//!
//! 根拠: RustCrypto `kem` crate (`Encapsulate`/`Decapsulate`), HPKE (RFC 9180)

use crate::algorithm::KemAlgorithm;
use crate::error::CryptoError;

/// 受信者の公開鍵に対してカプセル化を行う（送信者側）。
///
/// # 契約
/// - 各呼び出しは暗号学的に安全な乱数源から新しいエフェメラル鍵を生成しなければならない。
/// - 同じ `(shared_secret, encapsulated_key)` ペアを2回以上返してはならない。
/// - テスト用モック実装でこの契約に違反する場合は明示的に文書化すること。
pub trait Encapsulator: Send + Sync {
    /// このEncapsulatorが使用するアルゴリズム。
    fn algorithm(&self) -> KemAlgorithm;

    /// カプセル化。`(shared_secret, encapsulated_key)` を返す。
    ///
    /// - `shared_secret`: KEM出力。KDFの入力として使用される。
    /// - `encapsulated_key`: 受信者に送信するデータ（X25519: エフェメラル公開鍵）。
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError>;
}

/// カプセル化されたデータから共有秘密を復元する（受信者側）。
/// 秘密鍵を内部に保持し、外部に露出しない。
///
/// # 契約
/// - 決定論的: 同一の `encapsulated_key` に対し同一の `shared_secret` を返す。
pub trait Decapsulator: Send + Sync {
    /// このDecapsulatorが使用するアルゴリズム。
    fn algorithm(&self) -> KemAlgorithm;

    /// 公開鍵バイト列。
    fn public_key_bytes(&self) -> Vec<u8>;

    /// デカプセル化。`shared_secret` を返す。
    fn decapsulate(&self, encapsulated_key: &[u8]) -> Result<Vec<u8>, CryptoError>;
}
