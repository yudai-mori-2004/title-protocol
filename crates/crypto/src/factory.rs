// SPDX-License-Identifier: Apache-2.0

//! # Factory関数
//!
//! アルゴリズム識別子とバイト列から各traitオブジェクトを生成する。

use crate::algorithm::{AeadAlgorithm, KemAlgorithm, SigningAlgorithm};
use crate::error::CryptoError;
use crate::impls::aes256gcm::Aes256GcmAead;
use crate::impls::ed25519::{Ed25519Signer, Ed25519Verifier};
use crate::impls::x25519::{X25519Decapsulator, X25519Encapsulator};

/// 署名鍵を生成する。
///
/// - `seed`: アルゴリズム固有のseed長（Ed25519: 32バイト）。
pub fn create_signer(
    algorithm: SigningAlgorithm,
    seed: &[u8],
) -> Result<Box<dyn crate::signing::Signer>, CryptoError> {
    match algorithm {
        SigningAlgorithm::Ed25519 => {
            let arr: [u8; 32] =
                seed.try_into()
                    .map_err(|_| CryptoError::InvalidKeyLength {
                        expected: 32,
                        actual: seed.len(),
                    })?;
            Ok(Box::new(Ed25519Signer::from_seed(arr)))
        }
    }
}

/// 検証鍵を生成する。
pub fn create_verifier(
    algorithm: SigningAlgorithm,
    public_key: &[u8],
) -> Result<Box<dyn crate::signing::Verifier>, CryptoError> {
    match algorithm {
        SigningAlgorithm::Ed25519 => {
            let arr: [u8; 32] =
                public_key
                    .try_into()
                    .map_err(|_| CryptoError::InvalidKeyLength {
                        expected: 32,
                        actual: public_key.len(),
                    })?;
            Ok(Box::new(Ed25519Verifier::from_bytes(arr)?))
        }
    }
}

/// Decapsulatorを生成する。
pub fn create_decapsulator(
    algorithm: KemAlgorithm,
    seed: &[u8],
) -> Result<Box<dyn crate::kem::Decapsulator>, CryptoError> {
    match algorithm {
        KemAlgorithm::X25519HkdfSha256 => {
            let arr: [u8; 32] =
                seed.try_into()
                    .map_err(|_| CryptoError::InvalidKeyLength {
                        expected: 32,
                        actual: seed.len(),
                    })?;
            Ok(Box::new(X25519Decapsulator::from_seed(arr)))
        }
    }
}

/// Encapsulatorを生成する。
pub fn create_encapsulator(
    algorithm: KemAlgorithm,
    public_key: &[u8],
) -> Result<Box<dyn crate::kem::Encapsulator>, CryptoError> {
    match algorithm {
        KemAlgorithm::X25519HkdfSha256 => {
            let arr: [u8; 32] =
                public_key
                    .try_into()
                    .map_err(|_| CryptoError::InvalidKeyLength {
                        expected: 32,
                        actual: public_key.len(),
                    })?;
            Ok(Box::new(X25519Encapsulator::from_public_key(arr)))
        }
    }
}

/// AEADインスタンスを生成する。
pub fn create_aead(
    algorithm: AeadAlgorithm,
    key: &[u8],
) -> Result<Box<dyn crate::aead::Aead>, CryptoError> {
    match algorithm {
        AeadAlgorithm::Aes256Gcm => Ok(Box::new(Aes256GcmAead::from_key(key)?)),
    }
}
