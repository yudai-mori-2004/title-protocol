// SPDX-License-Identifier: Apache-2.0

//! # TEEエンドポイント
//!
//! 仕様書 §6.4

pub mod create_tree;
pub mod register_node;
pub mod sign;
pub mod verify;

#[cfg(test)]
pub(crate) mod test_helpers;

pub use create_tree::handle_create_tree;
pub use register_node::handle_register_node;
pub use sign::handle_sign;
pub use verify::handle_verify;

/// Base64エンジン（Standard）。
/// 全エンドポイントで共通使用。
pub(crate) fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}
