// SPDX-License-Identifier: Apache-2.0
//
// COSE_Sign1 (RFC 8152) parsing and signature verification.
// Origin: Amazon (aws-nitro-enclaves-cose) → Automata Network (RustCrypto port).

use std::collections::BTreeMap;

use anyhow::anyhow;
use serde::ser::SerializeSeq;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde_bytes::ByteBuf;
use serde_cbor::Value as CborValue;

use crate::sign::{verify_signature, PubKey, SigAlgo};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HeaderMap(
    #[serde(deserialize_with = "::serde_with::rust::maps_duplicate_key_is_error::deserialize")]
    BTreeMap<CborValue, CborValue>,
);

fn sig_algo_val(alg: SigAlgo) -> anyhow::Result<i8> {
    Ok(match alg {
        SigAlgo::EcdsaSHA256 => -7,
        SigAlgo::EcdsaSHA384 => -35,
        alg => return Err(anyhow!("unsupport sigAlgo: {:?}", alg)),
    })
}

#[derive(Debug)]
pub struct CoseSign1 {
    protected: ByteBuf,
    pub unprotected: HeaderMap,
    pub payload: ByteBuf,
    pub signature: ByteBuf,
}

impl CoseSign1 {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let cosesign1: serde_cbor::tags::Tagged<Self> = serde_cbor::from_slice(bytes)
            .map_err(|err| anyhow!("deserialization failed: {:?}", err))?;

        match cosesign1.tag {
            None | Some(18) => (),
            Some(tag) => return Err(anyhow!("tag error: {:?}", tag)),
        }
        let protected = cosesign1.value.protected.as_slice();
        let _: HeaderMap = serde_cbor::from_slice(protected)
            .map_err(|err| anyhow!("deserialization failed: {:?}", err))?;
        Ok(cosesign1.value)
    }

    pub fn verify_signature(&self, sig_algo: SigAlgo, issuer_key: PubKey) -> anyhow::Result<bool> {
        let protected: HeaderMap = serde_cbor::from_slice(&self.protected)
            .map_err(|err| anyhow!("deserialization failed: {:?}", err))?;

        if let Some(protected_signature_alg_val) = protected.0.get(&CborValue::Integer(1)) {
            let protected_signature_alg = match protected_signature_alg_val {
                CborValue::Integer(val) => val,
                _ => {
                    return Err(anyhow!(
                        "Protected Header contains invalid Signature Algorithm specification"
                    ))
                }
            };
            if protected_signature_alg != &(sig_algo_val(sig_algo)? as i128) {
                return Ok(false);
            }
        } else {
            return Err(anyhow!(
                "Protected Header does not contain a valid Signature Algorithm specification",
            ));
        }

        let sig_structure = SigStructure::new_sign1(&self.protected, &self.payload)?;
        let tbs = sig_structure.as_bytes()?;

        verify_signature(issuer_key, sig_algo, &self.signature, &tbs)
    }
}

impl Serialize for CoseSign1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(4))?;
        seq.serialize_element(&self.protected)?;
        seq.serialize_element(&self.unprotected)?;
        seq.serialize_element(&self.payload)?;
        seq.serialize_element(&self.signature)?;
        seq.end()
    }
}

impl<'de> Deserialize<'de> for CoseSign1 {
    fn deserialize<D>(deserializer: D) -> Result<CoseSign1, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{Error, SeqAccess, Visitor};
        use std::fmt;

        struct CoseSign1Visitor;

        impl<'de> Visitor<'de> for CoseSign1Visitor {
            type Value = CoseSign1;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a possibly tagged CoseSign1 structure")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<CoseSign1, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let protected = match seq.next_element()? {
                    Some(v) => v,
                    None => return Err(A::Error::missing_field("protected")),
                };
                let unprotected = match seq.next_element()? {
                    Some(v) => v,
                    None => return Err(A::Error::missing_field("unprotected")),
                };
                let payload = match seq.next_element()? {
                    Some(v) => v,
                    None => return Err(A::Error::missing_field("payload")),
                };
                let signature = match seq.next_element()? {
                    Some(v) => v,
                    None => return Err(A::Error::missing_field("signature")),
                };
                Ok(CoseSign1 {
                    protected,
                    unprotected,
                    payload,
                    signature,
                })
            }

            fn visit_newtype_struct<D>(self, deserializer: D) -> Result<CoseSign1, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_seq(CoseSign1Visitor)
            }
        }

        deserializer.deserialize_any(CoseSign1Visitor)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SigStructure(
    String,
    ByteBuf,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    Option<ByteBuf>,
    #[serde(default)] ByteBuf,
    #[serde(default)] ByteBuf,
);

impl SigStructure {
    pub fn new_sign1(body_protected: &[u8], payload: &[u8]) -> anyhow::Result<Self> {
        Ok(SigStructure(
            String::from("Signature1"),
            ByteBuf::from(body_protected.to_vec()),
            None,
            ByteBuf::new(),
            ByteBuf::from(payload.to_vec()),
        ))
    }

    pub fn as_bytes(&self) -> anyhow::Result<Vec<u8>> {
        serde_cbor::to_vec(self).map_err(|err| anyhow!("serialization failed: {:?}", err))
    }
}
