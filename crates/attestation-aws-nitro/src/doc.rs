// SPDX-License-Identifier: Apache-2.0
//
// AWS Nitro Attestation Document — parsing, certificate chain construction,
// and signature verification orchestration.
// Origin: Automata Network — aws-nitro-enclave-attestation (Apache-2.0).

use std::collections::BTreeMap;

use anyhow::{anyhow, Context};
use serde::Deserialize;
use serde_bytes::{ByteArray, ByteBuf};

use crate::cert::CertChain;
use crate::constants::AWS_NITRO_ROOT_CA_SHA256;
use crate::cose::CoseSign1;
use crate::sign::SigAlgo;

#[derive(Debug)]
pub struct AttestationReport {
    doc: AttestationDocument,
    cose_sign: CoseSign1,
}

impl AttestationReport {
    pub fn parse(document_data: &[u8]) -> anyhow::Result<Self> {
        let cose_sign = CoseSign1::from_bytes(document_data)
            .with_context(|| "AttestationReport::parse cose parse failed")?;
        let doc: AttestationDocument = serde_cbor::from_slice(&cose_sign.payload)
            .map_err(|err| anyhow!("document parse failed: {:?}", err))?;

        Ok(Self { doc, cose_sign })
    }

    pub fn doc(&self) -> &AttestationDocument {
        &self.doc
    }

    /// Consume the report, returning the parsed document.
    /// Available after `authenticate()` succeeds so callers can take ownership
    /// of the document fields without cloning.
    pub fn into_doc(self) -> AttestationDocument {
        self.doc
    }

    /// AWS Nitro Attestation Document authentication.
    /// <https://docs.aws.amazon.com/enclaves/latest/user/verify-root.html>
    ///
    /// 1. Build cert chain from cabundle + certificate
    /// 2. Pin the chain root to the AWS Nitro root CA SHA-256
    /// 3. Verify each cert against its parent (ECDSA P-384)
    /// 4. Check validity period against timestamp
    /// 5. Verify COSE_Sign1 with leaf cert public key (ES384)
    ///
    /// The `trusted_certs_len` parameter is retained for parity with the
    /// underlying [`CertChain`] API but should be set to 0 in production:
    /// the root pinning step (#2) is what establishes trust, so there is
    /// no reason to skip any chain link.
    pub fn authenticate(
        &self,
        trusted_certs_len: usize,
        timestamp: u64,
    ) -> anyhow::Result<CertChain<'_>> {
        let mut cert_chain = CertChain::new();
        for cert in &self.doc.cabundle {
            cert_chain.add_cert_by_der(cert)?;
        }
        cert_chain.add_cert_by_der(&self.doc.certificate)?;

        // Pin the chain root to the AWS Nitro Enclaves Root-G1 fingerprint.
        // Without this, an attacker can build a self-signed root that passes
        // every internal chain check.
        let root = cert_chain
            .certs
            .first()
            .ok_or_else(|| anyhow!("cabundle is empty"))?;
        if root.digest() != AWS_NITRO_ROOT_CA_SHA256 {
            return Err(anyhow!(
                "cabundle root does not match AWS Nitro Enclaves Root-G1 fingerprint"
            ));
        }

        match cert_chain.verify_chain(trusted_certs_len) {
            Ok(true) => {}
            Ok(false) => return Err(anyhow!("failed to verify x509 chain")),
            Err(err) => return Err(anyhow!("failed to verify x509 chain: {:?}", err)),
        };
        cert_chain.check_valid(timestamp)?;

        let pubkey = cert_chain.leaf_pubkey();
        let sig_algo = SigAlgo::EcdsaSHA384;

        let result = self.cose_sign.verify_signature(sig_algo, pubkey)?;
        if !result {
            return Err(anyhow!(
                "AttestationReport::authenticate invalid COSE signature for leaf key"
            ));
        }

        Ok(cert_chain)
    }
}

#[derive(Debug, Deserialize)]
pub struct AttestationDocument {
    pub module_id: String,
    pub timestamp: u64,
    pub digest: String,
    pub pcrs: BTreeMap<u64, ByteArray<48>>,
    pub certificate: ByteBuf,
    pub cabundle: Vec<ByteBuf>,
    pub public_key: Option<ByteBuf>,
    pub user_data: Option<ByteBuf>,
    pub nonce: Option<ByteBuf>,
}
