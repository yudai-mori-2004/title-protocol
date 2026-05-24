// SPDX-License-Identifier: Apache-2.0

//! # ML-KEM-768 KEM
//!
//! Spec §2.4 — suite_id: 0x03
//!
//! FIPS 203 Module-Lattice-Based KEM.
//! encap_key = 1088 bytes (KEM ciphertext).
//! public_key = 1184 bytes (encapsulation key).

use ml_kem::ml_kem_768;
use ml_kem::{Key, KeyExport};
use rand::RngCore;

use crate::CryptoError;

use super::{Decapsulator, Encapsulator};

const EK_SIZE: usize = 1184;
const CT_SIZE: usize = 1088;

/// Client-side ML-KEM-768 encapsulator.
pub struct MlKem768Encapsulator {
    ek: ml_kem_768::EncapsulationKey,
}

impl MlKem768Encapsulator {
    pub fn from_public_key(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != EK_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: EK_SIZE,
                actual: bytes.len(),
            });
        }
        let mut key = Key::<ml_kem_768::EncapsulationKey>::default();
        key.copy_from_slice(bytes);
        let ek = ml_kem_768::EncapsulationKey::new(&key)
            .map_err(|_| CryptoError::InvalidWireFormat("invalid ML-KEM-768 public key".into()))?;
        Ok(Self { ek })
    }
}

impl Encapsulator for MlKem768Encapsulator {
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        // Read 32 bytes from OsRng and feed them to encapsulate_deterministic
        // (rather than the rng-taking `Encapsulate::encapsulate`) so this
        // path is infallible: the deterministic variant returns
        // `(Ciphertext, SharedKey)` directly without an error case.
        let mut m = ml_kem::B32::default();
        rand::rngs::OsRng.fill_bytes(&mut m);
        let (ct, ss) = self.ek.encapsulate_deterministic(&m);
        Ok((ss.to_vec(), ct.to_vec()))
    }
}

/// TEE-side ML-KEM-768 decapsulator.
pub struct MlKem768Decapsulator {
    dk: ml_kem_768::DecapsulationKey,
}

impl MlKem768Decapsulator {
    /// Generate a new key pair from an RNG.
    /// Seeds 64 bytes from the RNG to derive the ML-KEM keypair deterministically.
    pub fn generate(rng: &mut (impl rand::RngCore + rand::CryptoRng)) -> Self {
        let mut seed_bytes = [0u8; 64];
        rng.fill_bytes(&mut seed_bytes);
        let mut seed = ml_kem::Seed::default();
        seed.copy_from_slice(&seed_bytes);
        let dk = ml_kem_768::DecapsulationKey::from_seed(seed);
        Self { dk }
    }
}

impl Decapsulator for MlKem768Decapsulator {
    fn public_key_bytes(&self) -> Vec<u8> {
        let ek: &ml_kem_768::EncapsulationKey = self.dk.encapsulation_key();
        let key: Key<ml_kem_768::EncapsulationKey> = KeyExport::to_bytes(ek);
        key.to_vec()
    }

    fn decapsulate(&self, encap_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if encap_key.len() != CT_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: CT_SIZE,
                actual: encap_key.len(),
            });
        }
        let mut ct_arr = ml_kem_768::Ciphertext::default();
        ct_arr.copy_from_slice(encap_key);
        let ss = ml_kem::Decapsulate::decapsulate(&self.dk, &ct_arr);
        Ok(ss.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let decap = MlKem768Decapsulator::generate(&mut rand::rngs::OsRng);
        let encap = MlKem768Encapsulator::from_public_key(&decap.public_key_bytes()).unwrap();

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = decap.decapsulate(&encap_key).unwrap();
        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn public_key_is_1184_bytes() {
        let decap = MlKem768Decapsulator::generate(&mut rand::rngs::OsRng);
        assert_eq!(decap.public_key_bytes().len(), EK_SIZE);
    }
}
