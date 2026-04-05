// SPDX-License-Identifier: Apache-2.0

//! # X25519 ECDH KEM実装
//!
//! 仕様書 §6.4
//!
//! X25519 ECDHをKEMとして抽象化する。
//! - Encapsulate: エフェメラル鍵ペア生成 → ECDH → (shared_secret, eph_pubkey)
//! - Decapsulate: ECDH(secret_key, eph_pubkey) → shared_secret

use x25519_dalek::{PublicKey, StaticSecret};

use crate::algorithm::KemAlgorithm;
use crate::error::CryptoError;
use crate::kem;

/// X25519 Encapsulator。受信者の公開鍵を保持。
pub struct X25519Encapsulator {
    recipient_pubkey: PublicKey,
}

impl X25519Encapsulator {
    /// 受信者の公開鍵（32バイト）から生成。
    pub fn from_public_key(bytes: [u8; 32]) -> Self {
        Self {
            recipient_pubkey: PublicKey::from(bytes),
        }
    }
}

impl kem::Encapsulator for X25519Encapsulator {
    fn algorithm(&self) -> KemAlgorithm {
        KemAlgorithm::X25519HkdfSha256
    }

    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let eph_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let eph_pubkey = PublicKey::from(&eph_secret);
        let shared = eph_secret.diffie_hellman(&self.recipient_pubkey);
        Ok((
            shared.as_bytes().to_vec(),
            eph_pubkey.as_bytes().to_vec(),
        ))
    }
}

/// X25519 Decapsulator。秘密鍵を内部に保持。
pub struct X25519Decapsulator {
    secret: StaticSecret,
    public: PublicKey,
}

impl X25519Decapsulator {
    /// 32バイトseedから生成。
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let secret = StaticSecret::from(seed);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }
}

impl kem::Decapsulator for X25519Decapsulator {
    fn algorithm(&self) -> KemAlgorithm {
        KemAlgorithm::X25519HkdfSha256
    }

    fn public_key_bytes(&self) -> Vec<u8> {
        self.public.as_bytes().to_vec()
    }

    fn decapsulate(&self, encapsulated_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let eph_pk_arr: [u8; 32] =
            encapsulated_key
                .try_into()
                .map_err(|_| CryptoError::InvalidKeyLength {
                    expected: 32,
                    actual: encapsulated_key.len(),
                })?;
        let eph_pubkey = PublicKey::from(eph_pk_arr);
        let shared = self.secret.diffie_hellman(&eph_pubkey);
        Ok(shared.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kem::{Decapsulator, Encapsulator};

    #[test]
    fn encapsulate_decapsulate_roundtrip() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(seed);
        let pk = decap.public_key_bytes();
        let pk_arr: [u8; 32] = pk.try_into().unwrap();
        let encap = X25519Encapsulator::from_public_key(pk_arr);

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = decap.decapsulate(&encap_key).unwrap();

        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn each_encapsulation_is_unique() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(seed);
        let pk_arr: [u8; 32] = decap.public_key_bytes().try_into().unwrap();
        let encap = X25519Encapsulator::from_public_key(pk_arr);

        let (shared1, ek1) = encap.encapsulate().unwrap();
        let (shared2, ek2) = encap.encapsulate().unwrap();

        assert_ne!(ek1, ek2, "encapsulated_key must differ per call");
        assert_ne!(shared1, shared2, "shared_secret must differ per call");
    }
}
