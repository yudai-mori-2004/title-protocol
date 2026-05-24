// SPDX-License-Identifier: Apache-2.0

//! # AWS Nitro Enclaves Runtime
//!
//! Spec §5.2
//!
//! `TeeRuntime` backed by the AWS Nitro Security Module (NSM). All operations
//! go through `/dev/nsm` via [aws-nitro-enclaves-nsm-api]; entropy and
//! attestation come from the enclave hypervisor, never from host RNG.
//!
//! The NSM device interface is abstracted behind the private [`NsmOps`] trait
//! so the runtime can be unit-tested against an in-memory implementation
//! without spinning up an actual enclave.
//!
//! [aws-nitro-enclaves-nsm-api]: https://docs.rs/aws-nitro-enclaves-nsm-api

use aws_nitro_enclaves_nsm_api::api::{Request, Response};
use aws_nitro_enclaves_nsm_api::driver;
use serde_bytes::ByteBuf;

use crate::{TeeError, TeeRuntime};

// ---------------------------------------------------------------------------
// NSM device abstraction
// ---------------------------------------------------------------------------

/// Operations exposed by the Nitro Security Module that the runtime needs.
/// Kept private — external callers go through [`NitroRuntime`].
trait NsmOps: Send + Sync {
    fn get_random(&self, len: usize) -> Result<Vec<u8>, TeeError>;
    fn get_attestation_doc(&self, user_data: Option<&[u8]>) -> Result<Vec<u8>, TeeError>;
}

/// Real NSM device backed by `/dev/nsm`. Only meaningful inside a Nitro Enclave.
struct RealNsm {
    fd: i32,
}

impl RealNsm {
    fn new() -> Result<Self, TeeError> {
        let fd = driver::nsm_init();
        if fd < 0 {
            return Err(TeeError::InitializationFailed(format!(
                "nsm_init returned {fd} (expected non-negative fd; is this running inside a Nitro Enclave?)"
            )));
        }
        Ok(Self { fd })
    }
}

impl Drop for RealNsm {
    fn drop(&mut self) {
        if self.fd >= 0 {
            driver::nsm_exit(self.fd);
        }
    }
}

impl NsmOps for RealNsm {
    fn get_random(&self, len: usize) -> Result<Vec<u8>, TeeError> {
        // NSM GetRandom returns up to ~256 bytes per call; loop until we have
        // the requested number of bytes.
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            match driver::nsm_process_request(self.fd, Request::GetRandom) {
                Response::GetRandom { random } => {
                    let take = (len - out.len()).min(random.len());
                    out.extend_from_slice(&random[..take]);
                }
                Response::Error(err) => {
                    return Err(TeeError::RandomFailed(format!(
                        "NSM GetRandom error: {err:?}"
                    )));
                }
                other => {
                    return Err(TeeError::RandomFailed(format!(
                        "NSM GetRandom unexpected response: {other:?}"
                    )));
                }
            }
        }
        Ok(out)
    }

    fn get_attestation_doc(&self, user_data: Option<&[u8]>) -> Result<Vec<u8>, TeeError> {
        let request = Request::Attestation {
            public_key: None,
            user_data: user_data.map(|d| ByteBuf::from(d.to_vec())),
            nonce: None,
        };
        match driver::nsm_process_request(self.fd, request) {
            Response::Attestation { document } => Ok(document),
            Response::Error(err) => Err(TeeError::AttestationFailed(format!(
                "NSM Attestation error: {err:?}"
            ))),
            other => Err(TeeError::AttestationFailed(format!(
                "NSM Attestation unexpected response: {other:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// NitroRuntime
// ---------------------------------------------------------------------------

/// AWS Nitro Enclaves runtime. Spec §5.2.
///
/// Construction calls `nsm_init`; both attestation and entropy then route
/// through the NSM device. The fd is closed on drop.
pub struct NitroRuntime {
    nsm: Box<dyn NsmOps>,
}

impl NitroRuntime {
    /// Initialise the runtime by opening `/dev/nsm`. Returns an error if NSM
    /// is not available (e.g. not running inside a Nitro Enclave).
    pub fn new() -> Result<Self, TeeError> {
        Ok(Self {
            nsm: Box::new(RealNsm::new()?),
        })
    }

    /// Construct with a custom NSM backend. For unit tests only.
    #[cfg(test)]
    fn with_nsm(nsm: Box<dyn NsmOps>) -> Self {
        Self { nsm }
    }
}

impl TeeRuntime for NitroRuntime {
    fn tee_type(&self) -> &str {
        title_attestation_aws_nitro::VENDOR
    }

    fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError> {
        let payload = if user_data.is_empty() {
            None
        } else {
            Some(user_data)
        };
        self.nsm.get_attestation_doc(payload)
    }

    fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.nsm.get_random(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// In-memory NSM stand-in for unit tests. Records calls and replays a
    /// canned attestation document.
    struct FakeNsm {
        random: RefCell<Vec<Vec<u8>>>,
        attestation: Vec<u8>,
        last_user_data: RefCell<Option<Vec<u8>>>,
    }

    // FakeNsm uses RefCell — fine for single-threaded tests. Mark Send + Sync
    // explicitly because the trait requires it.
    unsafe impl Send for FakeNsm {}
    unsafe impl Sync for FakeNsm {}

    impl NsmOps for FakeNsm {
        fn get_random(&self, len: usize) -> Result<Vec<u8>, TeeError> {
            self.random.borrow_mut().push(vec![0xAB; len]);
            Ok(vec![0xAB; len])
        }
        fn get_attestation_doc(&self, user_data: Option<&[u8]>) -> Result<Vec<u8>, TeeError> {
            *self.last_user_data.borrow_mut() = user_data.map(|d| d.to_vec());
            Ok(self.attestation.clone())
        }
    }

    fn fake_runtime(attestation: Vec<u8>) -> NitroRuntime {
        NitroRuntime::with_nsm(Box::new(FakeNsm {
            random: RefCell::new(Vec::new()),
            attestation,
            last_user_data: RefCell::new(None),
        }))
    }

    #[test]
    fn tee_type_matches_attestation_vendor_tag() {
        let rt = fake_runtime(vec![]);
        // Single source of truth for the "aws-nitro" identifier.
        assert_eq!(rt.tee_type(), title_attestation_aws_nitro::VENDOR);
    }

    #[test]
    fn random_bytes_returns_requested_length() {
        let rt = fake_runtime(vec![]);
        let bytes = rt.random_bytes(64).unwrap();
        assert_eq!(bytes.len(), 64);
    }

    #[test]
    fn random_bytes_zero_is_empty() {
        let rt = fake_runtime(vec![]);
        let bytes = rt.random_bytes(0).unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn attestation_doc_passes_through() {
        let rt = fake_runtime(b"fake-doc".to_vec());
        let doc = rt.get_attestation_document(b"user-data").unwrap();
        assert_eq!(doc, b"fake-doc");
    }
}
