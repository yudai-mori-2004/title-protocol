// SPDX-License-Identifier: Apache-2.0

//! # AES-256-GCM Authenticated Encryption
//!
//! Spec §2.4 — all suites use AES-256-GCM as the symmetric cipher.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};

type Nonce = aes_gcm::Nonce<aes_gcm::aead::consts::U12>;

use crate::CryptoError;

pub const NONCE_SIZE: usize = 12;
pub const KEY_SIZE: usize = 32;

/// Encrypt plaintext with AES-256-GCM.
pub fn encrypt(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != KEY_SIZE {
        return Err(CryptoError::InvalidKeyLength {
            expected: KEY_SIZE,
            actual: key.len(),
        });
    }
    if nonce.len() != NONCE_SIZE {
        return Err(CryptoError::InvalidWireFormat(format!(
            "nonce must be {} bytes, got {}",
            NONCE_SIZE,
            nonce.len()
        )));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::EncryptError)?;
    let nonce_arr: &[u8; NONCE_SIZE] = nonce.try_into().map_err(|_| {
        CryptoError::InvalidWireFormat("invalid nonce".into())
    })?;
    let nonce: &Nonce = nonce_arr.into();
    cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::EncryptError)
}

/// Decrypt ciphertext with AES-256-GCM.
pub fn decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != KEY_SIZE {
        return Err(CryptoError::InvalidKeyLength {
            expected: KEY_SIZE,
            actual: key.len(),
        });
    }
    if nonce.len() != NONCE_SIZE {
        return Err(CryptoError::InvalidWireFormat(format!(
            "nonce must be {} bytes, got {}",
            NONCE_SIZE,
            nonce.len()
        )));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::DecryptError)?;
    let nonce_arr: &[u8; NONCE_SIZE] = nonce.try_into().map_err(|_| {
        CryptoError::InvalidWireFormat("invalid nonce".into())
    })?;
    let nonce: &Nonce = nonce_arr.into();
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [0x42u8; KEY_SIZE];
        let nonce = [0x01u8; NONCE_SIZE];
        let plaintext = b"hello title protocol";

        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext);

        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key = [0x42u8; KEY_SIZE];
        let nonce = [0x01u8; NONCE_SIZE];
        let ciphertext = encrypt(&key, &nonce, b"data").unwrap();

        let wrong_key = [0x43u8; KEY_SIZE];
        assert!(decrypt(&wrong_key, &nonce, &ciphertext).is_err());
    }
}
