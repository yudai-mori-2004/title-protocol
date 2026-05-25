// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Core
//!
//! Processor trait, request/response types, input variants, error types,
//! streaming content abstractions.
//!
//! Spec §1.3, §2.2, §2.3, §3.1, §3.2, §4.3

mod c2pa_verify;
// `content_stream` は `pub` で外に出す。フラットな型 (`ContentSource` 等) は
// `pub use` で従来通り。`contract` サブモジュールに外部 crate のテストから
// `title_core::content_stream::contract::assert_content_source_contract` で
// アクセスできるようにするため、モジュールパス自体も公開する。
pub mod content_stream;
mod jumbf;
mod processor;
mod request;
mod response;
mod rootlens_license_v1;

pub use c2pa_verify::{
    compute_signature_hash, compute_signature_hash_from_manifest_data, C2paVerifyOutput,
    C2paVerifyProcessor, C2PA_VERIFY_PROCESSOR_ID,
};
pub use content_stream::{ContentSource, ContentStream, InMemorySource};
pub use processor::{Processor, ProcessorError, ProcessorRegistry};
pub use rootlens_license_v1::{
    RootLensBinding, RootLensLicenseV1Output, RootLensLicenseV1Processor,
    ROOTLENS_LICENSE_V1_PROCESSOR_ID,
};
pub use request::{EncryptedPayloadMetadata, EncryptionSuite, InputData, ProcessRequest};
pub use response::{ProcessResponse, ProcessorOutput, ProcessorStatus, VerifiableResponse};
