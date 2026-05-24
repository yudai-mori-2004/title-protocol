// SPDX-License-Identifier: Apache-2.0
//
// Constants used during AWS Nitro Attestation Document verification.

use x509_parser::der_parser::{oid, Oid};

/// SHA-256 of the AWS Nitro Enclaves Root-G1 certificate (DER form), published
/// at <https://aws-nitro-enclaves.amazonaws.com/AWS_NitroEnclaves_Root-G1.zip>.
/// Authentication of the chain root rests entirely on this fingerprint —
/// see <https://docs.aws.amazon.com/enclaves/latest/user/verify-root.html>.
pub const AWS_NITRO_ROOT_CA_SHA256: [u8; 32] = [
    0x64, 0x1a, 0x03, 0x21, 0xa3, 0xe2, 0x44, 0xef, 0xe4, 0x56, 0x46, 0x31, 0x95, 0xd6, 0x06, 0x31,
    0x7e, 0xd7, 0xcd, 0xcc, 0x3c, 0x17, 0x56, 0xe0, 0x98, 0x93, 0xf3, 0xc6, 0x8f, 0x79, 0xbb, 0x5b,
];

// EC public-key algorithm OID and named-curve parameter OIDs (RFC 5480).
pub const OID_KEY_ALGO_EC: Oid = oid!(1.2.840 .10045 .2 .1);
pub const EC_KEY_P256_OID: Oid = oid!(1.2.840 .10045 .3 .1 .7);
pub const EC_KEY_P384_OID: Oid = oid!(1.3.132 .0 .34);

// ECDSA signature OIDs (RFC 5758).
pub const OID_SIG_ALGO_ECDSA_SHA256: Oid = oid!(1.2.840 .10045 .4 .3 .2);
pub const OID_SIG_ALGO_ECDSA_SHA384: Oid = oid!(1.2.840 .10045 .4 .3 .3);
