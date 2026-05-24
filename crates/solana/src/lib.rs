// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Solana Extension
//!
//! Spec §6.1, §6.2
//!
//! Provides Solana-specific functionality for Title Protocol:
//! - Ed25519 signing key generation and management (TEE startup)
//! - Whitelist PDA types for on-chain key registration
//! - cNFT (Bubblegum V2) transaction construction and partial signing
//! - Solana Extension orchestration (offchain data verification → cNFT mint)

pub mod cnft;
pub mod extension;
pub mod signing_key;
pub mod whitelist;
