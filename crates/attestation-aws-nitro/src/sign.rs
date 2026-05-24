// SPDX-License-Identifier: Apache-2.0
//
// Signature verification dispatch — routes by OID to the appropriate RustCrypto crate.
// AWS Nitro chains only contain ECDSA-P256/P384 certificates, so RSA paths are
// intentionally omitted.

use std::borrow::Cow;

use crate::constants::*;
use anyhow::anyhow;
use p256::ecdsa::{
    signature::Verifier as P256Verifier, Signature as P256Signature,
    VerifyingKey as P256VerifyingKey,
};
use p384::ecdsa::{
    signature::hazmat::PrehashVerifier, Signature as P384Signature,
    VerifyingKey as P384VerifyingKey,
};
use sha2::{Digest, Sha256};
use x509_parser::der_parser::Oid;
use x509_parser::x509::AlgorithmIdentifier;

#[derive(Debug, PartialEq, Clone)]
pub struct PubKey<'a> {
    pub algo: KeyAlgo,
    pub val: &'a [u8],
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum KeyAlgo {
    EcdsaP256,
    EcdsaP384,
}

impl KeyAlgo {
    pub fn from_algo(algo: &AlgorithmIdentifier) -> anyhow::Result<Self> {
        if algo.oid() != &OID_KEY_ALGO_EC {
            return Err(anyhow!(
                "unsupported public-key algorithm: {:?}",
                algo.oid()
            ));
        }
        let params = algo
            .parameters
            .as_ref()
            .ok_or_else(|| anyhow!("ECDSA public key missing curve parameters"))?;
        // `Any.data` holds the raw OID content (no tag/length prefix), which
        // is exactly what `Oid::new` consumes.
        let curve_oid = Oid::new(Cow::Borrowed(params.data));
        if curve_oid == EC_KEY_P256_OID {
            Ok(Self::EcdsaP256)
        } else if curve_oid == EC_KEY_P384_OID {
            Ok(Self::EcdsaP384)
        } else {
            Err(anyhow!("unsupported ECDSA curve OID: {:?}", curve_oid))
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SigAlgo {
    EcdsaSHA256,
    EcdsaSHA384,
}

impl SigAlgo {
    pub fn from_oid(oid: &Oid) -> anyhow::Result<Self> {
        if oid == &OID_SIG_ALGO_ECDSA_SHA256 {
            Ok(SigAlgo::EcdsaSHA256)
        } else if oid == &OID_SIG_ALGO_ECDSA_SHA384 {
            Ok(SigAlgo::EcdsaSHA384)
        } else {
            Err(anyhow!(
                "unsupported signature OID: {:?}",
                oid.to_id_string()
            ))
        }
    }

    /// Compatibility table for DER-encoded X.509 signatures only — matches
    /// the combinations supported by [`verify_signature_der`]. COSE_Sign1
    /// raw signatures use a stricter table (only (P-256, SHA-256) and
    /// (P-384, SHA-384)); the corresponding check lives inside
    /// [`verify_signature_raw`] itself, since AWS Nitro never produces a
    /// mismatched COSE signature in practice.
    pub fn check_compatible_with_der(self, key_algo: KeyAlgo) -> anyhow::Result<()> {
        match (self, key_algo) {
            (SigAlgo::EcdsaSHA256, KeyAlgo::EcdsaP256)
            | (SigAlgo::EcdsaSHA256, KeyAlgo::EcdsaP384)
            | (SigAlgo::EcdsaSHA384, KeyAlgo::EcdsaP384) => Ok(()),
            _ => Err(anyhow!(
                "incompatible key/signature algorithms for DER ECDSA: key={:?}, sig={:?}",
                key_algo,
                self,
            )),
        }
    }
}

/// Verify a DER-encoded ECDSA signature (X.509 certificate signatures).
///
/// Malformed DER — negative INTEGER, indefinite length, trailing bytes — is
/// rejected by the curve crate's `Signature::from_der`, so no hand-rolled DER
/// walking is needed.
pub fn verify_signature_der(
    pubkey: PubKey,
    sig_algo: SigAlgo,
    sig_der: &[u8],
    msg: &[u8],
) -> anyhow::Result<bool> {
    match (pubkey.algo, sig_algo) {
        (KeyAlgo::EcdsaP256, SigAlgo::EcdsaSHA256) => {
            let vk = P256VerifyingKey::from_sec1_bytes(pubkey.val)
                .map_err(|err| anyhow!("parse P-256 verifying key: {err}"))?;
            let signature = P256Signature::from_der(sig_der)
                .map_err(|err| anyhow!("parse P-256 DER signature: {err}"))?;
            Ok(vk.verify(msg, &signature).is_ok())
        }
        (KeyAlgo::EcdsaP384, SigAlgo::EcdsaSHA256) => {
            let vk = P384VerifyingKey::from_sec1_bytes(pubkey.val)
                .map_err(|err| anyhow!("parse P-384 verifying key: {err}"))?;
            let signature = P384Signature::from_der(sig_der)
                .map_err(|err| anyhow!("parse P-384 DER signature: {err}"))?;
            let digest = Sha256::digest(msg);
            Ok(vk.verify_prehash(&digest, &signature).is_ok())
        }
        (KeyAlgo::EcdsaP384, SigAlgo::EcdsaSHA384) => {
            let vk = P384VerifyingKey::from_sec1_bytes(pubkey.val)
                .map_err(|err| anyhow!("parse P-384 verifying key: {err}"))?;
            let signature = P384Signature::from_der(sig_der)
                .map_err(|err| anyhow!("parse P-384 DER signature: {err}"))?;
            Ok(vk.verify(msg, &signature).is_ok())
        }
        (key_algo, sig_algo) => Err(anyhow!(
            "incompatible key/signature algorithms: key={:?}, sig={:?}",
            key_algo,
            sig_algo,
        )),
    }
}

/// Verify a fixed-length raw ECDSA signature (`r || s`) — COSE_Sign1 per RFC 8152.
pub fn verify_signature_raw(
    pubkey: PubKey,
    sig_algo: SigAlgo,
    sig_raw: &[u8],
    msg: &[u8],
) -> anyhow::Result<bool> {
    match (pubkey.algo, sig_algo) {
        (KeyAlgo::EcdsaP256, SigAlgo::EcdsaSHA256) => {
            let vk = P256VerifyingKey::from_sec1_bytes(pubkey.val)
                .map_err(|err| anyhow!("parse P-256 verifying key: {err}"))?;
            let signature = P256Signature::from_slice(sig_raw)
                .map_err(|err| anyhow!("parse P-256 raw signature: {err}"))?;
            Ok(vk.verify(msg, &signature).is_ok())
        }
        (KeyAlgo::EcdsaP384, SigAlgo::EcdsaSHA384) => {
            let vk = P384VerifyingKey::from_sec1_bytes(pubkey.val)
                .map_err(|err| anyhow!("parse P-384 verifying key: {err}"))?;
            let signature = P384Signature::from_slice(sig_raw)
                .map_err(|err| anyhow!("parse P-384 raw signature: {err}"))?;
            Ok(vk.verify(msg, &signature).is_ok())
        }
        (key_algo, sig_algo) => Err(anyhow!(
            "incompatible key/signature algorithms for raw ECDSA: key={:?}, sig={:?}",
            key_algo,
            sig_algo,
        )),
    }
}
