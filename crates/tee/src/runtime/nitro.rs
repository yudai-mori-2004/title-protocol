// SPDX-License-Identifier: Apache-2.0

//! # AWS Nitro Enclaves ランタイム実装
//!
//! 仕様書 §6.4
//!
//! AWS Nitro Enclaves上で動作するTEEランタイム。
//! NSM (Nitro Security Module) APIを使用して鍵生成とAttestation取得を行う。
//!
//! ## 設計
//!
//! NSMデバイス操作は `NsmOps` ���レイトで抽象化し、テスト時にはモック注入が可能。
//! - 本番（Linux/Nitro Enclave）: `RealNsm` — `/dev/nsm` 経由でNSM APIを呼び出し
//! - テスト: `MockNsm` — `OsRng` でエントロピー生成、モックAttestation返却

use std::sync::OnceLock;

use title_crypto::impls::ed25519::Ed25519Signer;
use title_crypto::impls::x25519::X25519Decapsulator;
use title_crypto::signing::Signer;
use title_crypto::kem::Decapsulator;
use title_crypto::{SigningAlgorithm, KemAlgorithm};

use super::TeeRuntime;

// ─────────────────────────────────────────────
// NSMデバイス操作の抽象化
// ─────────────────────────────────────────────

/// NSMデバイス操作の抽象化トレイト。
/// 仕様書 §6.4
trait NsmOps: Send + Sync {
    fn get_random(&self, len: usize) -> Vec<u8>;
    fn get_attestation_doc(
        &self,
        public_key: Option<&[u8]>,
        user_data: Option<&[u8]>,
        nonce: Option<&[u8]>,
    ) -> Vec<u8>;
}

// ─────────────────────────────────────────────
// 本番NSMデバイス（Linux/Nitro Enclaves）
// ─────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod real_nsm {
    use super::NsmOps;
    use aws_nitro_enclaves_nsm_api::api::{Request, Response};
    use aws_nitro_enclaves_nsm_api::driver;

    pub struct RealNsm {
        fd: i32,
    }

    impl RealNsm {
        pub fn new() -> Self {
            let fd = driver::nsm_init();
            if fd < 0 {
                panic!("NSMデバイス初期化に失敗しました (fd={fd})");
            }
            Self { fd }
        }
    }

    impl NsmOps for RealNsm {
        fn get_random(&self, len: usize) -> Vec<u8> {
            let response = driver::nsm_process_request(self.fd, Request::GetRandom);
            match response {
                Response::GetRandom { random } => {
                    assert!(random.len() >= len, "NSM GetRandom: 要求{len}B、取得{}B", random.len());
                    random[..len].to_vec()
                }
                resp => panic!("NSM GetRandom: 予期しないレスポンス: {resp:?}"),
            }
        }

        fn get_attestation_doc(
            &self,
            public_key: Option<&[u8]>,
            user_data: Option<&[u8]>,
            nonce: Option<&[u8]>,
        ) -> Vec<u8> {
            let response = driver::nsm_process_request(
                self.fd,
                Request::Attestation {
                    public_key: public_key.map(|k| k.into()),
                    user_data: user_data.map(|d| d.into()),
                    nonce: nonce.map(|n| n.into()),
                },
            );
            match response {
                Response::Attestation { document } => document,
                resp => panic!("NSM Attestation: 予期しないレスポンス: {resp:?}"),
            }
        }
    }
}

// ─────────────────────────────────────────────
// テスト用モックNSMデバイス
// ─────────────────────────────────────────────

#[cfg(test)]
mod mock_nsm {
    use super::NsmOps;

    pub struct MockNsm;

    impl NsmOps for MockNsm {
        fn get_random(&self, len: usize) -> Vec<u8> {
            use rand::RngCore;
            let mut buf = vec![0u8; len];
            rand::rngs::OsRng.fill_bytes(&mut buf);
            buf
        }

        fn get_attestation_doc(
            &self,
            public_key: Option<&[u8]>,
            user_data: Option<&[u8]>,
            nonce: Option<&[u8]>,
        ) -> Vec<u8> {
            let doc = serde_json::json!({
                "module_id": "nitro-mock",
                "digest": "SHA384",
                "timestamp": 0u64,
                "pcrs": {
                    "PCR0": vec![0u8; 48],
                    "PCR1": vec![0u8; 48],
                    "PCR2": vec![0u8; 48],
                },
                "public_key": public_key.map(|k| k.to_vec()),
                "user_data": user_data.map(|d| d.to_vec()),
                "nonce": nonce.map(|n| n.to_vec()),
            });
            serde_json::to_vec(&doc).expect("モックAttestation Documentのシリアライズに失敗")
        }
    }
}

// ─────────────────────────────────────────────
// NitroRuntime本体
// ─────────────────────────────────────────────

/// AWS Nitro Enclaves ランタイム。
/// 仕様書 §6.4
///
/// NSM APIを使用して鍵生成とAttestation取得を行う。
/// 全ての秘密鍵はEnclave内メモリにのみ保持され、外部にはエクスポートされない。
pub struct NitroRuntime {
    nsm: Box<dyn NsmOps>,
    signing: OnceLock<Ed25519Signer>,
    decap: OnceLock<X25519Decapsulator>,
    tree: OnceLock<Ed25519Signer>,
    ext_tree: OnceLock<Ed25519Signer>,
}

impl NitroRuntime {
    /// NitroRuntimeを初期化する（本番用）。
    #[cfg(target_os = "linux")]
    pub fn new() -> Self {
        Self {
            nsm: Box::new(real_nsm::RealNsm::new()),
            signing: OnceLock::new(),
            decap: OnceLock::new(),
            tree: OnceLock::new(),
            ext_tree: OnceLock::new(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        panic!(
            "NitroRuntimeはLinux (Nitro Enclave) 環境でのみ使用可能です。\
             ローカル開発にはMockRuntimeを使用してください。"
        )
    }

    #[cfg(test)]
    pub(crate) fn with_mock() -> Self {
        Self {
            nsm: Box::new(mock_nsm::MockNsm),
            signing: OnceLock::new(),
            decap: OnceLock::new(),
            tree: OnceLock::new(),
            ext_tree: OnceLock::new(),
        }
    }

    /// NSMエントロピーから32バイトseedを取得する。
    fn nsm_seed(&self) -> [u8; 32] {
        self.nsm.get_random(32)
            .try_into()
            .expect("NSMエントロピーは32バイトであるべき")
    }
}

impl TeeRuntime for NitroRuntime {
    fn tee_type(&self) -> &str {
        "aws_nitro"
    }

    fn generate_keypairs(&self) {
        let _ = self.signing.set(Ed25519Signer::from_seed(self.nsm_seed()));
        let _ = self.decap.set(X25519Decapsulator::from_seed(self.nsm_seed()));
        let _ = self.tree.set(Ed25519Signer::from_seed(self.nsm_seed()));
        let _ = self.ext_tree.set(Ed25519Signer::from_seed(self.nsm_seed()));
    }

    fn get_attestation(&self) -> Vec<u8> {
        let signing_pk = self.protocol_signer().public_key_bytes();
        let encryption_pk = self.decapsulator().public_key_bytes();
        self.nsm.get_attestation_doc(
            Some(&signing_pk),
            Some(&encryption_pk),
            None,
        )
    }

    fn protocol_signer(&self) -> &dyn Signer {
        self.signing.get().expect("鍵が未生成です")
    }

    fn solana_signer(&self) -> &dyn Signer {
        self.signing.get().expect("鍵が未生成です")
    }

    fn tree_signer(&self) -> &dyn Signer {
        self.tree.get().expect("鍵が未生成です")
    }

    fn ext_tree_signer(&self) -> &dyn Signer {
        self.ext_tree.get().expect("鍵が未生成です")
    }

    fn decapsulator(&self) -> &dyn Decapsulator {
        self.decap.get().expect("鍵が未生成です")
    }

    fn protocol_signing_algorithm(&self) -> SigningAlgorithm {
        SigningAlgorithm::Ed25519
    }

    fn kem_algorithm(&self) -> KemAlgorithm {
        KemAlgorithm::X25519HkdfSha256
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use title_crypto::Encapsulator;

    #[test]
    fn sign_verify_roundtrip() {
        let rt = NitroRuntime::with_mock();
        rt.generate_keypairs();

        let msg = b"Nitro test message";
        let sig = rt.protocol_signer().sign(msg).unwrap();
        let pubkey = rt.protocol_signer().public_key_bytes();

        let verifier = title_crypto::create_verifier(SigningAlgorithm::Ed25519, &pubkey).unwrap();
        assert!(verifier.verify(msg, &sig).is_ok());
    }

    #[test]
    fn kem_roundtrip() {
        let rt = NitroRuntime::with_mock();
        rt.generate_keypairs();

        let pk = rt.decapsulator().public_key_bytes();
        let pk_arr: [u8; 32] = pk.try_into().unwrap();
        let encap = title_crypto::impls::x25519::X25519Encapsulator::from_public_key(pk_arr);

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = rt.decapsulator().decapsulate(&encap_key).unwrap();
        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn tree_sign_verify() {
        let rt = NitroRuntime::with_mock();
        rt.generate_keypairs();

        let msg = b"Tree test";
        let sig = rt.tree_signer().sign(msg).unwrap();
        let pubkey = rt.tree_signer().public_key_bytes();

        let verifier = title_crypto::create_verifier(SigningAlgorithm::Ed25519, &pubkey).unwrap();
        assert!(verifier.verify(msg, &sig).is_ok());
    }

    #[test]
    fn attestation_document() {
        let rt = NitroRuntime::with_mock();
        rt.generate_keypairs();

        let attestation = rt.get_attestation();
        let doc: serde_json::Value = serde_json::from_slice(&attestation).unwrap();
        assert_eq!(doc["module_id"], "nitro-mock");
    }

    #[test]
    fn tee_type_is_aws_nitro() {
        let rt = NitroRuntime::with_mock();
        assert_eq!(rt.tee_type(), "aws_nitro");
    }
}
