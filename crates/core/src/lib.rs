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
//! - `c2pa_verify` — c2pa-verify processor + signature_hash utility (§3.2, §1.3)
//! - `jumbf` — JUMBF parser for COSE signature extraction
//! - `error` — `CoreError`

pub mod c2pa_verify;
pub mod error;
mod jumbf;
pub mod processor;
pub mod processor_outputs;
pub mod request;
pub mod response;

// Re-exports for convenience
pub use c2pa_verify::{
    compute_signature_hash, compute_signature_hash_from_manifest_data, C2paVerifyProcessor,
    C2PA_VERIFY_PROCESSOR_ID,
};
pub use error::CoreError;
pub use processor::{Processor, ProcessorError, ProcessorRegistry};
pub use request::{EncryptionSuite, EncryptedPayloadMetadata, InputData, ProcessRequest};
pub use response::{ProcessResponse, ProcessorOutput, ProcessorStatus, VerifiableResponse};
