// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol 暗号処理
//!
//! ## アーキテクチャ: 3ペアtrait + 合成関数
//!
//! | ペア | 秘密鍵側 | 公開鍵側 | 現在の実装 |
//! |------|---------|---------|-----------|
//! | 署名 | `Signer` | `Verifier` | Ed25519 |
//! | KEM | `Decapsulator` | `Encapsulator` | X25519 ECDH |
//! | AEAD | `Aead::decrypt` | `Aead::encrypt` | AES-256-GCM |
//!
//! 合成関数 `open_request` / `seal_for` がKEM+KDF+AEADを内部で合成する。
//! handlerはKEM, KDF, nonce, shared_secret, ワイヤーフォーマットを一切知らない。
//!
//! ## Attestation Document検証
//! `attestation` モジュールでVM型TEEのAttestation Document検証を提供する。
//! ベンダー実装はfeature flagで分離される。

// --- 3ペアtrait ---
pub mod signing;
pub mod kem;
pub mod aead;

// --- アルゴリズム・エラー型 ---
pub mod algorithm;
pub mod error;

// --- 合成関数・ユーティリティ ---
pub mod sealed_channel;
pub mod wire;
pub mod domain;
pub mod factory;

// --- 具象実装 ---
pub mod impls;

// --- Attestation ---
pub mod attestation;

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use error::CryptoError;
pub use algorithm::{SigningAlgorithm, KemAlgorithm, AeadAlgorithm, CryptoSuite};
pub use signing::{Signer, Verifier};
pub use kem::{Encapsulator, Decapsulator};
pub use aead::Aead;
pub use sealed_channel::{OpenedRequest, ResponseSealer, open_request, seal_for};
pub use domain::domain_tagged;
pub use factory::{create_signer, create_verifier, create_decapsulator, create_encapsulator, create_aead};

// ---------------------------------------------------------------------------
// ハッシュ関数（抽象化不要 — SHA-256は量子耐性あり）
// ---------------------------------------------------------------------------

use sha2::{Digest, Sha256};

/// SHA-256ハッシュ計算。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Active Manifestの署名からcontent_hashを計算する。
/// 仕様書 §2.1: `content_hash = SHA-256(Active Manifestの署名)`
pub fn content_hash_from_manifest_signature(manifest_signature: &[u8]) -> [u8; 32] {
    sha256(manifest_signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_value() {
        let hash = sha256(b"");
        assert_eq!(
            hex::encode(hash),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_content_hash_is_sha256_of_signature() {
        let signature = b"mock cose signature bytes";
        assert_eq!(content_hash_from_manifest_signature(signature), sha256(signature));
    }
}
