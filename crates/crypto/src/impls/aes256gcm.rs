// SPDX-License-Identifier: Apache-2.0

//! # AES-256-GCM AEAD実装
//!
//! 仕様書 §6.4
//!
//! AAD（Additional Authenticated Data）対応。RFC 5116準拠。

use aes_gcm::aead::Aead as AesGcmAeadTrait;
use aes_gcm::aead::Payload;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

use crate::algorithm::AeadAlgorithm;
use crate::error::CryptoError;

/// AES-256-GCM AEAD。対称鍵を内部に保持。
pub struct Aes256GcmAead {
    cipher: Aes256Gcm,
}

impl Aes256GcmAead {
    /// 32バイト鍵から生成。
    pub fn from_key(key: &[u8]) -> Result<Self, CryptoError> {
        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength {
                expected: 32,
                actual: key.len(),
            })?;
        Ok(Self { cipher })
    }
}

impl crate::aead::Aead for Aes256GcmAead {
    fn algorithm(&self) -> AeadAlgorithm {
        AeadAlgorithm::Aes256Gcm
    }

    fn nonce_size(&self) -> usize {
        12
    }

    fn encrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        if nonce.len() != 12 {
            return Err(CryptoError::InvalidWireFormat(format!(
                "AES-256-GCM nonce must be 12 bytes, got {}",
                nonce.len()
            )));
        }
        let nonce = Nonce::from_slice(nonce);
        let payload = Payload {
            msg: plaintext,
            aad,
        };
        self.cipher
            .encrypt(nonce, payload)
            .map_err(|_| CryptoError::EncryptError)
    }

    fn decrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        if nonce.len() != 12 {
            return Err(CryptoError::InvalidWireFormat(format!(
                "AES-256-GCM nonce must be 12 bytes, got {}",
                nonce.len()
            )));
        }
        let nonce = Nonce::from_slice(nonce);
        let payload = Payload {
            msg: ciphertext,
            aad,
        };
        self.cipher
            .decrypt(nonce, payload)
            .map_err(|_| CryptoError::DecryptError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aead::Aead;

    #[test]
    fn roundtrip() {
        let key = [1u8; 32];
        let aead = Aes256GcmAead::from_key(&key).unwrap();
        let nonce = [0u8; 12];
        let aad = b"context";
        let pt = b"hello world";

        let ct = aead.encrypt(&nonce, aad, pt).unwrap();
        let dec = aead.decrypt(&nonce, aad, &ct).unwrap();
        assert_eq!(dec, pt);
    }

    #[test]
    fn wrong_aad_fails() {
        let key = [1u8; 32];
        let aead = Aes256GcmAead::from_key(&key).unwrap();
        let nonce = [0u8; 12];

        let ct = aead.encrypt(&nonce, b"aad1", b"secret").unwrap();
        assert!(aead.decrypt(&nonce, b"aad2", &ct).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let aead1 = Aes256GcmAead::from_key(&[1u8; 32]).unwrap();
        let aead2 = Aes256GcmAead::from_key(&[2u8; 32]).unwrap();
        let nonce = [0u8; 12];

        let ct = aead1.encrypt(&nonce, &[], b"secret").unwrap();
        assert!(aead2.decrypt(&nonce, &[], &ct).is_err());
    }
}
