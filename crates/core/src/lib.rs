// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Core
//!
//! Processor trait, request/response types, input variants, error types.
//!
//! Spec §1.3, §2.2, §2.3, §3.1, §3.2

mod c2pa_verify;
mod jumbf;
mod processor;
mod request;
mod response;

pub use c2pa_verify::{
    compute_signature_hash, compute_signature_hash_from_manifest_data, C2paAction,
    C2paVerifyOutput, C2paVerifyProcessor, SignerInfo, C2PA_VERIFY_PROCESSOR_ID,
};
pub use processor::{Processor, ProcessorError, ProcessorRegistry};
pub use request::{EncryptedPayloadMetadata, EncryptionSuite, InputData, ProcessRequest};
pub use response::{ProcessResponse, ProcessorOutput, ProcessorStatus, VerifiableResponse};
