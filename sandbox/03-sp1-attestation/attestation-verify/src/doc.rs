// Attestation Document パース・認証
//
// AWS Nitro Attestation Document の COSE_Sign1 パース、
// 証明書チェーン構築、署名検証の orchestration。
// 原著: Automata Network (Apache-2.0)

use std::collections::BTreeMap;

use anyhow::{anyhow, Context};
use serde::Deserialize;
use serde_bytes::{ByteArray, ByteBuf};

use crate::cert::CertChain;
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
            .with_context(|| "AttestationDocument::authenticate parse failed")?;
        let doc: AttestationDocument = serde_cbor::from_slice(&cose_sign.payload)
            .map_err(|err| anyhow!("document parse failed: {:?}", err))?;

        Ok(Self { doc, cose_sign })
    }

    pub fn doc(&self) -> &AttestationDocument {
        &self.doc
    }

    /// AWS Nitro Attestation Document の認証。
    /// https://docs.aws.amazon.com/enclaves/latest/user/verify-root.html
    ///
    /// 1. cabundle + certificate から証明書チェーンを構築
    /// 2. 各証明書の署名を親証明書の公開鍵で検証（ECDSA P-384）
    /// 3. 証明書の有効期限を timestamp で検証
    /// 4. リーフ証明書の公開鍵で COSE_Sign1 署名を検証（ES384）
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
                "AttestationDocument::authenticate invalid COSE certificate for provided key"
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
