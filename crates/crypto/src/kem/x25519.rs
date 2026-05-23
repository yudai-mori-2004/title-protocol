// SPDX-License-Identifier: Apache-2.0

//! # X25519 ECDH KEM
//!
//! Spec §2.4 — suite_id: 0x01
//!
//! X25519 Diffie-Hellman key exchange.
//! encap_key = 32 bytes (ephemeral public key).

use x25519_dalek::{PublicKey, StaticSecret};

use crate::CryptoError;

use super::{Decapsulator, Encapsulator};

/// Client-side X25519 encapsulator.
pub struct X25519Encapsulator {
    recipient_pubkey: PublicKey,
}

impl X25519Encapsulator {
    pub fn from_public_key(bytes: &[u8]) -> Result<Self, CryptoError> {
        let arr: [u8; 32] =
            bytes
                .try_into()
                .map_err(|_| CryptoError::InvalidKeyLength {
                    expected: 32,
                    actual: bytes.len(),
                })?;
        Ok(Self {
            recipient_pubkey: PublicKey::from(arr),
        })
    }
}

impl Encapsulator for X25519Encapsulator {
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let eph_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let eph_pubkey = PublicKey::from(&eph_secret);
        let shared = eph_secret.diffie_hellman(&self.recipient_pubkey);
        Ok((shared.as_bytes().to_vec(), eph_pubkey.as_bytes().to_vec()))
    }
}

/// TEE-side X25519 decapsulator.
pub struct X25519Decapsulator {
    secret: StaticSecret,
    public: PublicKey,
}

impl X25519Decapsulator {
    pub fn from_seed(seed: &[u8]) -> Result<Self, CryptoError> {
        let arr: [u8; 32] =
            seed.try_into()
                .map_err(|_| CryptoError::InvalidKeyLength {
                    expected: 32,
                    actual: seed.len(),
                })?;
        let secret = StaticSecret::from(arr);
        let public = PublicKey::from(&secret);
        Ok(Self { secret, public })
    }
}

impl Decapsulator for X25519Decapsulator {
    fn public_key_bytes(&self) -> Vec<u8> {
        self.public.as_bytes().to_vec()
    }

    fn decapsulate(&self, encap_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let arr: [u8; 32] =
            encap_key
                .try_into()
                .map_err(|_| CryptoError::InvalidKeyLength {
                    expected: 32,
                    actual: encap_key.len(),
                })?;
        let eph_pubkey = PublicKey::from(arr);
        let shared = self.secret.diffie_hellman(&eph_pubkey);
        Ok(shared.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(&seed).unwrap();
        let encap = X25519Encapsulator::from_public_key(&decap.public_key_bytes()).unwrap();

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = decap.decapsulate(&encap_key).unwrap();
        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn each_encapsulation_unique() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(&seed).unwrap();
        let encap = X25519Encapsulator::from_public_key(&decap.public_key_bytes()).unwrap();

        let (s1, ek1) = encap.encapsulate().unwrap();
        let (s2, ek2) = encap.encapsulate().unwrap();
        assert_ne!(ek1, ek2);
        assert_ne!(s1, s2);
    }
}
