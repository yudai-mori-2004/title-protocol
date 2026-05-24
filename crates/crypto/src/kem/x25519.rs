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
        let arr: [u8; 32] = bytes
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
        reject_zero_shared_secret(shared.as_bytes())?;
        Ok((shared.as_bytes().to_vec(), eph_pubkey.as_bytes().to_vec()))
    }
}

/// TEE-side X25519 decapsulator.
pub struct X25519Decapsulator {
    secret: StaticSecret,
    public: PublicKey,
}

impl X25519Decapsulator {
    pub fn generate(rng: &mut (impl rand::RngCore + rand::CryptoRng)) -> Self {
        let secret = StaticSecret::random_from_rng(rng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }
}

impl Decapsulator for X25519Decapsulator {
    fn public_key_bytes(&self) -> Vec<u8> {
        self.public.as_bytes().to_vec()
    }

    fn decapsulate(&self, encap_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let arr: [u8; 32] = encap_key
            .try_into()
            .map_err(|_| CryptoError::InvalidKeyLength {
                expected: 32,
                actual: encap_key.len(),
            })?;
        let eph_pubkey = PublicKey::from(arr);
        let shared = self.secret.diffie_hellman(&eph_pubkey);
        reject_zero_shared_secret(shared.as_bytes())?;
        Ok(shared.as_bytes().to_vec())
    }
}

/// RFC 7748 §6.1: implementations MAY check for the all-zero output that
/// signals the peer sent a low-order point (a contributory-behavior attack
/// where one side can force the shared secret regardless of the other key).
/// `x25519-dalek` does not clamp this for us; reject here.
fn reject_zero_shared_secret(shared: &[u8]) -> Result<(), CryptoError> {
    let mut acc = 0u8;
    for &b in shared {
        acc |= b;
    }
    if acc == 0 {
        return Err(CryptoError::InvalidWireFormat(
            "X25519 produced all-zero shared secret (low-order point)".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let decap = X25519Decapsulator::generate(&mut rand::rngs::OsRng);
        let encap = X25519Encapsulator::from_public_key(&decap.public_key_bytes()).unwrap();

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = decap.decapsulate(&encap_key).unwrap();
        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn low_order_point_rejected() {
        // X25519 low-order point: all-zero u-coordinate (the identity).
        let decap = X25519Decapsulator::generate(&mut rand::rngs::OsRng);
        let zero_pubkey = [0u8; 32];
        assert!(decap.decapsulate(&zero_pubkey).is_err());
    }

    #[test]
    fn each_encapsulation_unique() {
        let decap = X25519Decapsulator::generate(&mut rand::rngs::OsRng);
        let encap = X25519Encapsulator::from_public_key(&decap.public_key_bytes()).unwrap();

        let (s1, ek1) = encap.encapsulate().unwrap();
        let (s2, ek2) = encap.encapsulate().unwrap();
        assert_ne!(ek1, ek2);
        assert_ne!(s1, s2);
    }
}
