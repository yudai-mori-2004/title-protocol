// SPDX-License-Identifier: Apache-2.0
//
// X.509 certificate chain construction and verification.

use crate::sign::{verify_signature_der, KeyAlgo, PubKey, SigAlgo};

use anyhow::{anyhow, Context};
use sha2::Sha256;
use x509_parser::prelude::*;

#[derive(Debug, PartialEq, Clone)]
pub struct Cert<'a> {
    pub raw: X509Certificate<'a>,
    pub bytes: &'a [u8],
    pubkey_algo: KeyAlgo,
}

impl<'a> Cert<'a> {
    pub fn parse_der(bytes: &'a [u8]) -> anyhow::Result<Self> {
        let (remain, raw) = X509Certificate::from_der(bytes)
            .map_err(|err| anyhow!("parse cert failed: {:?}", err))?;
        if !remain.is_empty() {
            return Err(anyhow!("parse cert not consume all bytes"));
        }
        let pubkey_algo = {
            let info = raw.public_key();
            KeyAlgo::from_algo(&info.algorithm)?
        };
        Ok(Self {
            raw,
            bytes,
            pubkey_algo,
        })
    }

    pub fn check_valid(&self, time: ASN1Time) -> anyhow::Result<()> {
        let validity = &self.raw.validity;
        if !validity.is_valid_at(time) {
            Err(anyhow!(
                "certificate is not valid at time: {}({}), range: {}({}) - {}({})",
                time,
                time.timestamp(),
                validity.not_before,
                validity.not_before.timestamp(),
                validity.not_after,
                validity.not_after.timestamp(),
            ))
        } else {
            Ok(())
        }
    }

    pub fn digest(&self) -> [u8; 32] {
        sha256(self.bytes)
    }

    pub fn sig_algo(&self) -> anyhow::Result<SigAlgo> {
        SigAlgo::from_oid(self.raw.signature_algorithm.oid())
    }

    pub fn pubkey(&self) -> PubKey<'_> {
        PubKey {
            algo: self.pubkey_algo,
            val: self.raw.public_key().subject_public_key.as_ref(),
        }
    }

    pub fn signature(&self) -> &[u8] {
        self.raw.signature_value.as_ref()
    }

    pub fn tbs_certificate(&self) -> &[u8] {
        self.raw.tbs_certificate.as_ref()
    }

    /// Verify this certificate's signature with the issuer's public key.
    pub fn verify(&self, issuer: &Self) -> anyhow::Result<bool> {
        let issuer_key = issuer.pubkey();
        let sig_algo = self.sig_algo()?;
        sig_algo.check_compatible_with(issuer_key.algo)?;
        verify_signature_der(issuer_key, sig_algo, self.signature(), self.tbs_certificate())
    }
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    Sha256::digest(bytes).into()
}

pub struct CertChain<'a> {
    /// Certificates ordered root → leaf. The root must be authenticated out of
    /// band (the AWS Nitro verifier pins its SHA-256 fingerprint).
    pub certs: Vec<Cert<'a>>,
}

impl<'a> Default for CertChain<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> CertChain<'a> {
    pub fn new() -> Self {
        Self { certs: Vec::new() }
    }

    pub fn add_cert_by_der<'b: 'a>(&mut self, buf: &'b [u8]) -> anyhow::Result<()> {
        let cert = Cert::parse_der(buf)?;
        self.certs.push(cert);
        Ok(())
    }

    pub fn leaf_pubkey(&self) -> PubKey<'_> {
        self.leaf().pubkey()
    }

    pub fn leaf(&self) -> &Cert<'a> {
        &self.certs[self.certs.len() - 1]
    }

    pub fn check_valid(&self, timestamp: u64) -> anyhow::Result<()> {
        let time = ASN1Time::from_timestamp(timestamp as i64)
            .map_err(|_| anyhow!("invalid timestamp: {}", timestamp))?;
        if self.certs.is_empty() {
            return Err(anyhow!("cert chain is empty"));
        }
        for (idx, cert) in self.certs.iter().enumerate() {
            cert.check_valid(time).with_context(|| {
                format!("cert not valid at chain [{}/{}]", idx + 1, self.certs.len())
            })?;
        }
        Ok(())
    }

    /// Verify each non-root certificate against its parent. The caller is
    /// responsible for authenticating the root (e.g. SHA-256 pin).
    pub fn verify_chain(&self) -> anyhow::Result<bool> {
        if self.certs.is_empty() {
            return Err(anyhow!("cert chain is empty"));
        }
        for i in 1..self.certs.len() {
            let subject = &self.certs[i];
            let issuer = &self.certs[i - 1];
            if !subject
                .verify(issuer)
                .with_context(|| format!("verify cert sig failed at {i}"))?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
