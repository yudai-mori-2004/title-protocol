// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Encryption Primitives
//!
//! Spec §2.4
//!
//! Client-to-TEE content encryption with three cipher suites,
//! direction-separated key derivation, and binary wire format.
//!
//! ## Modules
//!
//! - `error` — Error types
//! - `kem` — Key Encapsulation Mechanism (X25519, P-256, ML-KEM-768)
//! - `aead` — AES-256-GCM authenticated encryption
//! - `hkdf` — Direction-separated key derivation (HKDF-SHA256)
//! - `wire` — Binary wire format (request + response)
//! - `payload` — Encrypted payload internal structure
//! - `key_bundle` — Per-suite key pair generation at TEE startup
//! - `sealed_channel` — High-level encrypt/decrypt composition

pub mod aead;
pub mod error;
pub mod hkdf;
pub mod kem;
pub mod key_bundle;
pub mod payload;
pub mod sealed_channel;
pub mod wire;

pub use error::CryptoError;
pub use key_bundle::KeyBundle;
pub use sealed_channel::{open_request, seal_for, OpenedRequest};
