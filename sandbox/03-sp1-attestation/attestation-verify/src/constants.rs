// OID 定数
//
// X.509 証明書チェーン検証で使用するアルゴリズム識別子。
// 原著: Automata Network (Apache-2.0)

use x509_parser::der_parser::{oid, Oid};

// Key Algo OIDs
pub const OID_KEY_ALGO_EC: Oid = oid!(1.2.840 .10045 .2 .1);
pub const OID_KEY_ALGO_PKCS1_V1_5: Oid = oid!(1.2.840 .113549 .1 .1 .1);

pub const EC_KEY_P256_PARAM_OID: &str = "1.2.840.10045.3.1.7";
pub const EC_KEY_P384_PARAM_OID: &str = "1.3.132.0.34";

// Signature Algo OIDs
pub const OID_SIG_ALGO_ECDSA_SHA256: Oid = oid!(1.2.840 .10045 .4 .3 .2);
pub const OID_SIG_ALGO_ECDSA_SHA384: Oid = oid!(1.2.840 .10045 .4 .3 .3);
pub const OID_SIG_ALGO_RSASSA_PSS: Oid = oid!(1.2.840 .113549 .1 .1 .10);
pub const OID_SIG_ALGO_RSA_SHA256: Oid = oid!(1.2.840 .113549 .1 .1 .11);
