// SPDX-License-Identifier: Apache-2.0

//! # Gatewayエンドポイント
//!
//! 仕様書 §6.2

pub mod health;
pub mod upload_url;
pub mod verify;
pub mod sign;
pub mod sign_and_mint;

pub use health::handle_health;
pub use upload_url::handle_upload_url;
pub use verify::handle_verify;
pub use sign::handle_sign;
pub use sign_and_mint::handle_sign_and_mint;
#[cfg(test)]
pub(crate) use sign_and_mint::{SignAndMintInput, SignAndMintItem};
