// Attestation Document 検証ライブラリ
//
// COSE_Sign1 パース、X.509 証明書チェーン検証、ECDSA P-384 署名検証を行う。
// 暗号プリミティブは全て RustCrypto 標準クレートに委譲。
//
// 本コードは Automata Network の aws-nitro-enclave-attestation (Apache-2.0) を基に、
// 不要な依存を除去し内部化したもの。
// 原著作権: Copyright Amazon.com, Inc. (COSE パース部分)
//           Copyright Automata Network (検証フロー・X.509 チェーン部分)
// ライセンス: Apache-2.0

// SP1 precompile パッチ版のクレートを標準名でシャドウ
#[cfg(feature = "sp1")]
extern crate sha2_sp1 as sha2;

#[cfg(feature = "sp1")]
extern crate p256_sp1 as p256;

pub mod constants;
pub mod sign;
pub mod cert;
pub mod cose;
pub mod doc;

pub use doc::{AttestationDocument, AttestationReport};
pub use cert::CertChain;
pub use sign::{SigAlgo, PubKey};
