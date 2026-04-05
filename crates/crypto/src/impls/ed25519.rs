// SPDX-License-Identifier: Apache-2.0

//! # Ed25519 署名/検証実装
//!
//! 仕様書 §5.1, §6.4

use ed25519_dalek::{
    Signer as DalekSigner,
    SigningKey, VerifyingKey,
};

use crate::algorithm::SigningAlgorithm;
use crate::error::CryptoError;
use crate::signing;

/// Ed25519署名鍵。秘密鍵を内部に保持。
pub struct Ed25519Signer {
    key: SigningKey,
}

impl Ed25519Signer {
    /// 32バイトseedから生成。
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&seed),
        }
    }
}

impl signing::Signer for Ed25519Signer {
    fn algorithm(&self) -> SigningAlgorithm {
        SigningAlgorithm::Ed25519
    }

    fn public_key_bytes(&self) -> Vec<u8> {
        self.key.verifying_key().to_bytes().to_vec()
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let sig = self.key.sign(message);
        Ok(sig.to_bytes().to_vec())
    }
}

/// Ed25519検証鍵。
pub struct Ed25519Verifier {
    key: VerifyingKey,
}

impl Ed25519Verifier {
    /// 32バイト公開鍵から生成。
    pub fn from_bytes(bytes: [u8; 32]) -> Result<Self, CryptoError> {
        let key = VerifyingKey::from_bytes(&bytes)
            .map_err(|e| CryptoError::InvalidKeyFormat(e.to_string()))?;
        Ok(Self { key })
    }
}

impl signing::Verifier for Ed25519Verifier {
    fn algorithm(&self) -> SigningAlgorithm {
        SigningAlgorithm::Ed25519
    }

    fn public_key_bytes(&self) -> Vec<u8> {
        self.key.to_bytes().to_vec()
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        let sig_arr: [u8; 64] = signature
            .try_into()
            .map_err(|_| CryptoError::InvalidKeyLength {
                expected: 64,
                actual: signature.len(),
            })?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
        self.key
            .verify_strict(message, &sig)
            .map_err(|_| CryptoError::SignatureVerifyError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{Signer, Verifier};

    #[test]
    fn roundtrip() {
        let seed: [u8; 32] = rand::random();
        let signer = Ed25519Signer::from_seed(seed);
        let pubkey = signer.public_key_bytes();
        let pk_arr: [u8; 32] = pubkey.try_into().unwrap();
        let verifier = Ed25519Verifier::from_bytes(pk_arr).unwrap();

        let msg = b"test message";
        let sig = signer.sign(msg).unwrap();
        assert!(verifier.verify(msg, &sig).is_ok());
    }

    #[test]
    fn wrong_message_fails() {
        let seed: [u8; 32] = rand::random();
        let signer = Ed25519Signer::from_seed(seed);
        let pubkey = signer.public_key_bytes();
        let pk_arr: [u8; 32] = pubkey.try_into().unwrap();
        let verifier = Ed25519Verifier::from_bytes(pk_arr).unwrap();

        let sig = signer.sign(b"original").unwrap();
        assert!(verifier.verify(b"tampered", &sig).is_err());
    }
}
