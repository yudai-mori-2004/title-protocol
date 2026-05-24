// SPDX-License-Identifier: Apache-2.0

//! # P-256 ECDH KEM
//!
//! Spec §2.4 — suite_id: 0x02
//!
//! ECDH on NIST P-256.
//! encap_key = 65 bytes (uncompressed SEC1 point).

use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};

use crate::CryptoError;

use super::{Decapsulator, Encapsulator};

/// Client-side P-256 encapsulator.
pub struct P256Encapsulator {
    recipient_pubkey: PublicKey,
}

impl P256Encapsulator {
    pub fn from_public_key(bytes: &[u8]) -> Result<Self, CryptoError> {
        let pk = PublicKey::from_sec1_bytes(bytes).map_err(|_| CryptoError::InvalidKeyLength {
            expected: 65,
            actual: bytes.len(),
        })?;
        Ok(Self {
            recipient_pubkey: pk,
        })
    }
}

impl Encapsulator for P256Encapsulator {
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let eph = EphemeralSecret::random(&mut rand::rngs::OsRng);
        let eph_public = eph.public_key();
        let shared = eph.diffie_hellman(&self.recipient_pubkey);
        Ok((
            shared.raw_secret_bytes().to_vec(),
            eph_public.to_encoded_point(false).as_bytes().to_vec(),
        ))
    }
}

/// TEE-side P-256 decapsulator.
pub struct P256Decapsulator {
    secret: SecretKey,
}

impl P256Decapsulator {
    /// Generate a new key pair from an RNG using SEC1 rejection sampling.
    ///
    /// `SecretKey::random` rejects scalars outside `[1, n-1]` so the resulting
    /// secret is uniform on the valid scalar range, unlike `from_bytes` which
    /// errors on the unlucky draws and would otherwise leak bias if the caller
    /// retried with the next bytes.
    pub fn generate(rng: &mut (impl rand::RngCore + rand::CryptoRng)) -> Self {
        Self {
            secret: SecretKey::random(rng),
        }
    }
}

impl Decapsulator for P256Decapsulator {
    fn public_key_bytes(&self) -> Vec<u8> {
        self.secret
            .public_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec()
    }

    fn decapsulate(&self, encap_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let eph_pk =
            PublicKey::from_sec1_bytes(encap_key).map_err(|_| CryptoError::InvalidKeyLength {
                expected: 65,
                actual: encap_key.len(),
            })?;
        let shared =
            p256::ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), eph_pk.as_affine());
        Ok(shared.raw_secret_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let decap = P256Decapsulator::generate(&mut rand::rngs::OsRng);
        let encap = P256Encapsulator::from_public_key(&decap.public_key_bytes()).unwrap();

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = decap.decapsulate(&encap_key).unwrap();
        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn public_key_is_65_bytes() {
        let decap = P256Decapsulator::generate(&mut rand::rngs::OsRng);
        assert_eq!(decap.public_key_bytes().len(), 65);
    }
}
