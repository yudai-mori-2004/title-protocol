// SPDX-License-Identifier: Apache-2.0

//! # TEE Runtime Implementations
//!
//! Spec §5.2
//!
//! Concrete implementations of the `TeeRuntime` trait.
//!
//! - `mock` — Local development / testing (OsRng + mock attestation)
//! - `vendor::aws` — AWS Nitro Enclaves (feature `vendor-aws`)

pub mod mock;
