// SPDX-License-Identifier: Apache-2.0

//! # TEEベンダー実装
//!
//! 仕様書 §5.2
//!
//! feature flagでベンダー固有のTeeRuntime実装を切り替える。
//!
//! ## 対応ベンダー
//!
//! | ベンダー | Feature flag | 推奨実装 |
//! |---|---|---|
//! | AWS Nitro Enclaves | `vendor-aws` | `NitroRuntime` |
//! | AMD SEV-SNP | (未実装) | — |
//! | Intel TDX | (未実装) | — |

#[cfg(feature = "vendor-aws")]
pub mod aws;
