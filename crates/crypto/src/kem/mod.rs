// SPDX-License-Identifier: Apache-2.0

//! # Key Encapsulation Mechanism
//!
//! Spec §2.4
//!
//! Three KEM suites: X25519, P-256 ECDH, ML-KEM-768.
//! Each provides Encapsulator (client) and Decapsulator (TEE) roles.

pub mod ml_kem768;
pub mod p256_ecdh;
pub mod x25519;

use title_core::EncryptionSuite;

use crate::CryptoError;

/// Client-side KEM: generates (shared_secret, encap_key).
/// Spec §2.4
pub trait Encapsulator: Send + Sync {
    /// Perform KEM encapsulation.
    /// Returns (shared_secret, encap_key).
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError>;
}

/// TEE-side KEM: recovers shared_secret from encap_key.
/// Spec §2.4
pub trait Decapsulator: Send + Sync {
    /// Public key bytes for Gateway notification.
    fn public_key_bytes(&self) -> Vec<u8>;

    /// Recover shared_secret from encap_key.
    fn decapsulate(&self, encap_key: &[u8]) -> Result<Vec<u8>, CryptoError>;
}

/// Expected encap_key length for each suite.
/// Spec §2.4 wire format.
pub fn encap_key_len(suite: EncryptionSuite) -> usize {
    match suite {
        EncryptionSuite::X25519 => 32,
        EncryptionSuite::P256 => 65,
        EncryptionSuite::MlKem768 => 1088,
    }
}

/// Create an Encapsulator from a public key and suite.
pub fn create_encapsulator(
    suite: EncryptionSuite,
    public_key: &[u8],
) -> Result<Box<dyn Encapsulator>, CryptoError> {
    match suite {
        EncryptionSuite::X25519 => {
            Ok(Box::new(x25519::X25519Encapsulator::from_public_key(public_key)?))
        }
        EncryptionSuite::P256 => {
            Ok(Box::new(p256_ecdh::P256Encapsulator::from_public_key(public_key)?))
        }
        EncryptionSuite::MlKem768 => {
            Ok(Box::new(ml_kem768::MlKem768Encapsulator::from_public_key(public_key)?))
        }
    }
}
