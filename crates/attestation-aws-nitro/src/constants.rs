// SPDX-License-Identifier: Apache-2.0
//
// Constants used during AWS Nitro Attestation Document verification.

use x509_parser::der_parser::{oid, Oid};

// ----------------------------------------------------------------------------
// AWS Nitro root CA pinning
// ----------------------------------------------------------------------------

/// SHA-256 of the AWS Nitro Enclaves Root-G1 certificate (DER form).
///
/// Published by AWS at:
/// <https://aws-nitro-enclaves.amazonaws.com/AWS_NitroEnclaves_Root-G1.zip>
///
/// AWS's official [verify-root] page recommends explicit fingerprint matching
/// at application start. Without this check, an attacker can build a forged
/// cabundle rooted at their own self-signed certificate and still pass the
/// chain's pairwise signature checks — every link is mathematically valid,
/// the root is just wrong.
///
/// The 30-year root has been stable since launch; if AWS ever rotates it,
/// this constant must be updated and a new version of the verifier shipped.
///
/// [verify-root]: https://docs.aws.amazon.com/enclaves/latest/user/verify-root.html
pub const AWS_NITRO_ROOT_CA_SHA256: [u8; 32] = [
    0x64, 0x1a, 0x03, 0x21, 0xa3, 0xe2, 0x44, 0xef, 0xe4, 0x56, 0x46, 0x31, 0x95, 0xd6, 0x06, 0x31,
    0x7e, 0xd7, 0xcd, 0xcc, 0x3c, 0x17, 0x56, 0xe0, 0x98, 0x93, 0xf3, 0xc6, 0x8f, 0x79, 0xbb, 0x5b,
];

// ----------------------------------------------------------------------------
// OID constants used during certificate chain verification.
// Origin: Automata Network — aws-nitro-enclave-attestation (Apache-2.0).
// ----------------------------------------------------------------------------

// Key algorithm OIDs
pub const OID_KEY_ALGO_EC: Oid = oid!(1.2.840 .10045 .2 .1);
pub const OID_KEY_ALGO_PKCS1_V1_5: Oid = oid!(1.2.840 .113549 .1 .1 .1);

pub const EC_KEY_P256_PARAM_OID: &str = "1.2.840.10045.3.1.7";
pub const EC_KEY_P384_PARAM_OID: &str = "1.3.132.0.34";

// Signature algorithm OIDs
pub const OID_SIG_ALGO_ECDSA_SHA256: Oid = oid!(1.2.840 .10045 .4 .3 .2);
pub const OID_SIG_ALGO_ECDSA_SHA384: Oid = oid!(1.2.840 .10045 .4 .3 .3);
pub const OID_SIG_ALGO_RSASSA_PSS: Oid = oid!(1.2.840 .113549 .1 .1 .10);
pub const OID_SIG_ALGO_RSA_SHA256: Oid = oid!(1.2.840 .113549 .1 .1 .11);
