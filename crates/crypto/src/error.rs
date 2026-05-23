// SPDX-License-Identifier: Apache-2.0

//! # Crypto Error Types
//!
//! Spec §2.4

/// Cryptographic operation error.
/// Spec §2.4
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("ECDH key exchange failed")]
    EcdhError,

    #[error("HKDF key derivation failed: {0}")]
    HkdfError(String),

    #[error("Encryption failed")]
    EncryptError,

    #[error("Decryption failed")]
    DecryptError,

    #[error("Invalid key length: expected {expected}, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },

    #[error("Invalid wire format: {0}")]
    InvalidWireFormat(String),

    #[error("Unsupported suite: 0x{0:02x}")]
    UnsupportedSuite(u8),

    #[error("Invalid payload format: {0}")]
    InvalidPayload(String),
}
