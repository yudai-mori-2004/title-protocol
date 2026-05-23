// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Core
//!
//! Processor trait、リクエスト/レスポンス型、入力タイプ、エラー型を提供する。
//!
//! 仕様書 §1.3, §2.2, §2.3, §3.1, §3.2
//!
//! ## クレート構成
//!
//! - `request` — `ProcessRequest`, `InputData`, `EncryptionSuite` (§2.2, §2.4)
//! - `response` — `ProcessResponse`, `ProcessorOutput`, `VerifiableResponse` (§2.3)
//! - `processor` — `Processor` trait, `ProcessorError`, `ProcessorRegistry` (§3.1)
//! - `processor_outputs` — 各processor固有の出力型 (§3.2)
//! - `error` — `CoreError`

pub mod error;
pub mod processor;
pub mod processor_outputs;
pub mod request;
pub mod response;

// Re-exports for convenience
pub use error::CoreError;
pub use processor::{Processor, ProcessorError, ProcessorRegistry};
pub use request::{EncryptionSuite, EncryptedPayloadMetadata, InputData, ProcessRequest};
pub use response::{ProcessResponse, ProcessorOutput, ProcessorStatus, VerifiableResponse};
