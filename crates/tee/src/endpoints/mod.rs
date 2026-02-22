//! # TEEエンドポイント
//!
//! 仕様書 §6.4

pub mod create_tree;
pub mod sign;
pub mod verify;

#[cfg(test)]
pub(crate) mod test_helpers;

pub use create_tree::handle_create_tree;
pub use sign::handle_sign;
pub use verify::handle_verify;
