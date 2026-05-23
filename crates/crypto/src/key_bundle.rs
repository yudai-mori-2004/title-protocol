// SPDX-License-Identifier: Apache-2.0

//! # Key Bundle
//!
//! Spec §2.4
//!
//! Per-suite key pair generation at TEE startup.
//! Secret keys exist only in TEE memory.

use std::collections::HashMap;

use base64::Engine;
use title_core::EncryptionSuite;

use crate::kem::ml_kem768::MlKem768Decapsulator;
use crate::kem::p256_ecdh::P256Decapsulator;
use crate::kem::x25519::X25519Decapsulator;
use crate::kem::Decapsulator;
use crate::CryptoError;

/// TEE key bundle holding key pairs for all suites.
/// Spec §2.4
///
/// Generated at TEE startup; lost on restart.
pub struct KeyBundle {
    x25519: X25519Decapsulator,
    p256: P256Decapsulator,
    ml_kem: MlKem768Decapsulator,
}

impl KeyBundle {
    /// Generate key pairs for all suites.
    /// Spec §2.4 — TEE startup key generation.
    ///
    /// The RNG should be backed by TeeRuntime::random_bytes in production.
    pub fn generate(rng: &mut (impl rand::RngCore + rand::CryptoRng)) -> Result<Self, CryptoError> {
        let mut x25519_seed = [0u8; 32];
        rng.fill_bytes(&mut x25519_seed);
        let x25519 = X25519Decapsulator::from_seed(&x25519_seed)?;

        let mut p256_seed = [0u8; 32];
        rng.fill_bytes(&mut p256_seed);
        let p256 = P256Decapsulator::from_seed(&p256_seed)?;

        let ml_kem = MlKem768Decapsulator::generate(rng);

        Ok(Self {
            x25519,
            p256,
            ml_kem,
        })
    }

    /// Public keys as suite_name → Base64.
    /// Spec §2.5 — for GET /keys response.
    pub fn public_keys(&self) -> HashMap<String, String> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut keys = HashMap::new();
        keys.insert("x25519".into(), b64.encode(self.x25519.public_key_bytes()));
        keys.insert("p256".into(), b64.encode(self.p256.public_key_bytes()));
        keys.insert(
            "ml-kem-768".into(),
            b64.encode(self.ml_kem.public_key_bytes()),
        );
        keys
    }

    /// Decapsulate an encap_key for the given suite.
    pub fn decapsulate(
        &self,
        suite: EncryptionSuite,
        encap_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        match suite {
            EncryptionSuite::X25519 => self.x25519.decapsulate(encap_key),
            EncryptionSuite::P256 => self.p256.decapsulate(encap_key),
            EncryptionSuite::MlKem768 => self.ml_kem.decapsulate(encap_key),
        }
    }

    /// Raw public key bytes for a suite (not Base64).
    pub fn public_key_bytes(&self, suite: EncryptionSuite) -> Vec<u8> {
        match suite {
            EncryptionSuite::X25519 => self.x25519.public_key_bytes(),
            EncryptionSuite::P256 => self.p256.public_key_bytes(),
            EncryptionSuite::MlKem768 => self.ml_kem.public_key_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_public_keys() {
        let bundle = KeyBundle::generate(&mut rand::rngs::OsRng).unwrap();
        let keys = bundle.public_keys();

        assert!(keys.contains_key("x25519"));
        assert!(keys.contains_key("p256"));
        assert!(keys.contains_key("ml-kem-768"));

        assert_eq!(bundle.public_key_bytes(EncryptionSuite::X25519).len(), 32);
        assert_eq!(bundle.public_key_bytes(EncryptionSuite::P256).len(), 65);
        assert_eq!(
            bundle.public_key_bytes(EncryptionSuite::MlKem768).len(),
            1184
        );
    }
}
