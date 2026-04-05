// SPDX-License-Identifier: Apache-2.0

//! # ローカル開発用モックランタイム
//!
//! 仕様書 §6.4
//!
//! TEEハードウェアが利用できない開発環境で使用するモック実装。
//! メモリ内で鍵を生��し、固定のAttestation Documentを返す。

use std::sync::OnceLock;

use title_crypto::impls::ed25519::Ed25519Signer;
use title_crypto::impls::x25519::X25519Decapsulator;
use title_crypto::signing::Signer;
use title_crypto::kem::Decapsulator;
use title_crypto::{SigningAlgorithm, KemAlgorithm};

use super::TeeRuntime;

/// モックAttestation Documentの構造体。
/// 仕様書 §5.2 Step 4.1
#[derive(serde::Serialize)]
struct MockAttestationDocument {
    module_id: String,
    pcr0: Vec<u8>,
    pcr1: Vec<u8>,
    pcr2: Vec<u8>,
    signing_pubkey: Vec<u8>,
    encryption_pubkey: Vec<u8>,
}

/// モックTEEランタイム。ローカル開発・テスト用。
/// 仕様書 §6.4
///
/// 鍵は `OnceLock` で保持。`generate_keypairs()` で一度だけ生成し、以後不変。
pub struct MockRuntime {
    /// Protocol署名用 / Solana TX署名用（Phase 1では同一鍵）
    signing: OnceLock<Ed25519Signer>,
    /// KEM Decapsulator
    decap: OnceLock<X25519Decapsulator>,
    /// Core Tree署名用
    tree: OnceLock<Ed25519Signer>,
    /// Extension Tree署名用
    ext_tree: OnceLock<Ed25519Signer>,
}

impl MockRuntime {
    pub fn new() -> Self {
        Self {
            signing: OnceLock::new(),
            decap: OnceLock::new(),
            tree: OnceLock::new(),
            ext_tree: OnceLock::new(),
        }
    }
}

impl TeeRuntime for MockRuntime {
    fn tee_type(&self) -> &str {
        "mock"
    }

    fn generate_keypairs(&self) {
        let _ = self.signing.set(Ed25519Signer::from_seed(rand::random()));
        let _ = self.decap.set(X25519Decapsulator::from_seed(rand::random()));
        let _ = self.tree.set(Ed25519Signer::from_seed(rand::random()));
        let _ = self.ext_tree.set(Ed25519Signer::from_seed(rand::random()));
    }

    fn get_attestation(&self) -> Vec<u8> {
        let signing_pubkey = self.protocol_signer().public_key_bytes();
        let encryption_pubkey = self.decapsulator().public_key_bytes();

        let doc = MockAttestationDocument {
            module_id: "mock-tee".to_string(),
            pcr0: vec![0u8; 48],
            pcr1: vec![0u8; 48],
            pcr2: vec![0u8; 48],
            signing_pubkey,
            encryption_pubkey,
        };

        serde_json::to_vec(&doc).expect("MockAttestationDocumentのシリアライズに失敗")
    }

    fn protocol_signer(&self) -> &dyn Signer {
        self.signing.get().expect("鍵が未生成です")
    }

    fn solana_signer(&self) -> &dyn Signer {
        // Phase 1: protocol_signerと同一鍵
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
        let rt = MockRuntime::new();
        rt.generate_keypairs();

        let msg = b"Title Protocol test message";
        let sig = rt.protocol_signer().sign(msg).unwrap();
        let pubkey = rt.protocol_signer().public_key_bytes();

        let verifier = title_crypto::create_verifier(
            SigningAlgorithm::Ed25519,
            &pubkey,
        ).unwrap();
        assert!(verifier.verify(msg, &sig).is_ok());
    }

    #[test]
    fn kem_roundtrip() {
        let rt = MockRuntime::new();
        rt.generate_keypairs();

        let pk = rt.decapsulator().public_key_bytes();
        let pk_arr: [u8; 32] = pk.try_into().unwrap();
        let encap = title_crypto::impls::x25519::X25519Encapsulator::from_public_key(pk_arr);

        let (shared_enc, encap_key) = encap.encapsulate().unwrap();
        let shared_dec = rt.decapsulator().decapsulate(&encap_key).unwrap();
        assert_eq!(shared_enc, shared_dec);
    }

    #[test]
    fn sealed_channel_roundtrip() {
        let rt = MockRuntime::new();
        rt.generate_keypairs();

        let pk = rt.decapsulator().public_key_bytes();
        let pk_arr: [u8; 32] = pk.try_into().unwrap();
        let encap = title_crypto::impls::x25519::X25519Encapsulator::from_public_key(pk_arr);

        let aad = b"/verify";
        let plaintext = b"hello from client";

        let (wire, client_channel) = title_crypto::seal_for(&encap, plaintext, aad).unwrap();
        let opened = title_crypto::open_request(rt.decapsulator(), &wire, aad).unwrap();
        assert_eq!(opened.plaintext, plaintext);

        let response = b"signed json response";
        let resp_wire = opened.channel.seal(response, aad).unwrap();
        let resp_plain = client_channel.open(&resp_wire, aad).unwrap();
        assert_eq!(resp_plain, response);
    }

    #[test]
    fn attestation_document_structure() {
        let rt = MockRuntime::new();
        rt.generate_keypairs();

        let attestation = rt.get_attestation();
        let doc: serde_json::Value = serde_json::from_slice(&attestation).unwrap();

        assert_eq!(doc["module_id"], "mock-tee");
        let pcr0: Vec<u8> = serde_json::from_value(doc["pcr0"].clone()).unwrap();
        assert_eq!(pcr0.len(), 48);
        assert!(pcr0.iter().all(|&b| b == 0));
    }

    #[test]
    fn tee_type_is_mock() {
        let rt = MockRuntime::new();
        assert_eq!(rt.tee_type(), "mock");
    }

    #[test]
    fn protocol_and_solana_signer_share_key() {
        let rt = MockRuntime::new();
        rt.generate_keypairs();
        assert_eq!(
            rt.protocol_signer().public_key_bytes(),
            rt.solana_signer().public_key_bytes(),
        );
    }

    #[test]
    #[should_panic(expected = "鍵が未生成です")]
    fn sign_without_keypair_panics() {
        let rt = MockRuntime::new();
        let _ = rt.protocol_signer();
    }
}
