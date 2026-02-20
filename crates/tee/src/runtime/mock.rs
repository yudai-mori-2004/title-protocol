//! # ローカル開発用モックランタイム
//!
//! 仕様書 §6.4
//!
//! TEEハードウェアが利用できない開発環境で使用するモック実装。
//! メモリ内で鍵を生成し、固定のAttestation Documentを返す。

use std::sync::RwLock;

use ed25519_dalek::{Signer, SigningKey};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use super::TeeRuntime;

/// モックAttestation Documentの構造体。
/// 仕様書 §5.2 Step 4.1
///
/// Nitro Enclaveのdebug-modeと同等（PCR値が全てゼロ）。
#[derive(serde::Serialize)]
struct MockAttestationDocument {
    /// モジュールID
    module_id: String,
    /// PCR0（エンクレーブイメージ測定値）— debug-modeでは全ゼロ（48バイト）
    pcr0: Vec<u8>,
    /// PCR1（カーネル測定値）— debug-modeでは全ゼロ（48バイト）
    pcr1: Vec<u8>,
    /// PCR2（アプリケーション測定値）— debug-modeでは全ゼロ（48バイト）
    pcr2: Vec<u8>,
    /// 署名用公開鍵
    signing_pubkey: Vec<u8>,
    /// 暗号化用公開鍵
    encryption_pubkey: Vec<u8>,
}

/// モックTEEランタイム。ローカル開発・テスト用。
/// 仕様書 §6.4
pub struct MockRuntime {
    /// Ed25519署名用キーペア（メモリ内生成）
    signing_key: RwLock<Option<SigningKey>>,
    /// X25519暗号化用秘密鍵（メモリ内生成）
    encryption_secret: RwLock<Option<StaticSecret>>,
    /// Tree用Ed25519キーペア（メモリ内生成）
    tree_key: RwLock<Option<SigningKey>>,
}

impl MockRuntime {
    /// MockRuntimeを初期化する。
    pub fn new() -> Self {
        Self {
            signing_key: RwLock::new(None),
            encryption_secret: RwLock::new(None),
            tree_key: RwLock::new(None),
        }
    }
}

impl TeeRuntime for MockRuntime {
    /// モックランタイムのTEE種別を返す。
    fn tee_type(&self) -> &str {
        "mock"
    }

    /// メモリ内でEd25519署名用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    fn generate_signing_keypair(&self) {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let mut guard = self.signing_key.write().unwrap();
        *guard = Some(signing_key);
    }

    /// メモリ内でX25519暗号化用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    fn generate_encryption_keypair(&self) {
        let secret = StaticSecret::random_from_rng(&mut rand::rngs::OsRng);
        let mut guard = self.encryption_secret.write().unwrap();
        *guard = Some(secret);
    }

    /// 固定のモックAttestation Documentを返す。
    /// 仕様書 §5.2 Step 4.1
    ///
    /// PCR値は全てゼロ（Nitroのdebug-modeと同等）。
    fn get_attestation(&self) -> Vec<u8> {
        let signing_pubkey = self.signing_pubkey();
        let encryption_pubkey = self.encryption_pubkey();

        let doc = MockAttestationDocument {
            module_id: "mock-enclave".to_string(),
            pcr0: vec![0u8; 48],
            pcr1: vec![0u8; 48],
            pcr2: vec![0u8; 48],
            signing_pubkey,
            encryption_pubkey,
        };

        serde_json::to_vec(&doc).expect("MockAttestationDocumentのシリアライズに失敗")
    }

    /// 保持しているEd25519秘密鍵でデータに署名する。
    /// 仕様書 §5.1 Step 4
    fn sign(&self, message: &[u8]) -> Vec<u8> {
        let guard = self.signing_key.read().unwrap();
        let key = guard.as_ref().expect("署名用キーペアが未生成です");
        let signature = key.sign(message);
        signature.to_bytes().to_vec()
    }

    /// 署名用公開鍵（Ed25519 VerifyingKey）をバイト列で返す。
    /// 仕様書 §6.4
    fn signing_pubkey(&self) -> Vec<u8> {
        let guard = self.signing_key.read().unwrap();
        let key = guard.as_ref().expect("署名用キーペアが未生成です");
        key.verifying_key().to_bytes().to_vec()
    }

    /// 暗号化用秘密鍵（X25519 StaticSecret）のバイト列を返す。
    /// 仕様書 §6.4
    fn encryption_secret_key(&self) -> Vec<u8> {
        let guard = self.encryption_secret.read().unwrap();
        let secret = guard.as_ref().expect("暗号化用キーペアが未生成です");
        secret.to_bytes().to_vec()
    }

    /// 暗号化用公開鍵（X25519 PublicKey）のバイト列を返す。
    /// 仕様書 §6.4
    fn encryption_pubkey(&self) -> Vec<u8> {
        let guard = self.encryption_secret.read().unwrap();
        let secret = guard.as_ref().expect("暗号化用キーペアが未生成です");
        let pubkey = X25519PublicKey::from(secret);
        pubkey.to_bytes().to_vec()
    }

    /// メモリ内でTree用Ed25519キーペアを生成する。
    /// 仕様書 §6.4 Step 2
    fn generate_tree_keypair(&self) {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let mut guard = self.tree_key.write().unwrap();
        *guard = Some(key);
    }

    /// Tree用公開鍵（Ed25519 VerifyingKey）をバイト列で返す。
    /// 仕様書 §6.4 Step 2
    fn tree_pubkey(&self) -> Vec<u8> {
        let guard = self.tree_key.read().unwrap();
        let key = guard.as_ref().expect("Tree用キーペアが未生成です");
        key.verifying_key().to_bytes().to_vec()
    }

    /// Tree用秘密鍵でデータに署名する。
    /// 仕様書 §6.4 Step 2
    fn tree_sign(&self, message: &[u8]) -> Vec<u8> {
        let guard = self.tree_key.read().unwrap();
        let key = guard.as_ref().expect("Tree用キーペアが未生成です");
        let signature = key.sign(message);
        signature.to_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

    /// 鍵ペア生成→署名→検証のラウンドトリップテスト
    #[test]
    fn test_sign_verify_roundtrip() {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();

        let message = b"Title Protocol test message";
        let sig_bytes = rt.sign(message);
        let pubkey_bytes = rt.signing_pubkey();

        // 公開鍵と署名をデシリアライズして検証
        let verifying_key =
            VerifyingKey::from_bytes(&pubkey_bytes.try_into().expect("公開鍵は32バイト"))
                .expect("有効なEd25519公開鍵");
        let signature =
            Signature::from_bytes(&sig_bytes.try_into().expect("署名は64バイト"));

        assert!(verifying_key.verify(message, &signature).is_ok());
    }

    /// 不正なメッセージで署名検証が失敗することを確認
    #[test]
    fn test_sign_verify_wrong_message() {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();

        let sig_bytes = rt.sign(b"correct message");
        let pubkey_bytes = rt.signing_pubkey();

        let verifying_key =
            VerifyingKey::from_bytes(&pubkey_bytes.try_into().expect("公開鍵は32バイト"))
                .expect("有効なEd25519公開鍵");
        let signature =
            Signature::from_bytes(&sig_bytes.try_into().expect("署名は64バイト"));

        assert!(verifying_key.verify(b"wrong message", &signature).is_err());
    }

    /// 暗号化用鍵ペアのECDH鍵交換（共通鍵導出の一致確認）
    #[test]
    fn test_ecdh_key_agreement() {
        let rt = MockRuntime::new();
        rt.generate_encryption_keypair();

        let tee_secret_bytes: [u8; 32] = rt
            .encryption_secret_key()
            .try_into()
            .expect("秘密鍵は32バイト");
        let tee_pubkey_bytes: [u8; 32] = rt
            .encryption_pubkey()
            .try_into()
            .expect("公開鍵は32バイト");

        // クライアント側のエフェメラルキーペアを生成
        let client_secret = StaticSecret::random_from_rng(&mut rand::rngs::OsRng);
        let client_pubkey = X25519PublicKey::from(&client_secret);

        // TEE側: ECDH(tee_sk, client_pk)
        let tee_secret = StaticSecret::from(tee_secret_bytes);
        let shared_tee = tee_secret.diffie_hellman(&client_pubkey);

        // クライアント側: ECDH(client_sk, tee_pk)
        let tee_pubkey = X25519PublicKey::from(tee_pubkey_bytes);
        let shared_client = client_secret.diffie_hellman(&tee_pubkey);

        // 両者の共有秘密が一致することを確認
        assert_eq!(shared_tee.as_bytes(), shared_client.as_bytes());
    }

    /// Attestation Documentが正しい構造を持つことを確認
    #[test]
    fn test_attestation_document() {
        let rt = MockRuntime::new();
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();

        let attestation = rt.get_attestation();
        let doc: serde_json::Value =
            serde_json::from_slice(&attestation).expect("有効なJSON");

        assert_eq!(doc["module_id"], "mock-enclave");
        // PCR値が全てゼロ（48バイトのゼロ配列）
        let pcr0: Vec<u8> = serde_json::from_value(doc["pcr0"].clone()).unwrap();
        assert_eq!(pcr0.len(), 48);
        assert!(pcr0.iter().all(|&b| b == 0));

        // 公開鍵が含まれていることを確認
        let signing_pk: Vec<u8> =
            serde_json::from_value(doc["signing_pubkey"].clone()).unwrap();
        assert_eq!(signing_pk.len(), 32);
        assert_eq!(signing_pk, rt.signing_pubkey());

        let enc_pk: Vec<u8> =
            serde_json::from_value(doc["encryption_pubkey"].clone()).unwrap();
        assert_eq!(enc_pk.len(), 32);
        assert_eq!(enc_pk, rt.encryption_pubkey());
    }

    /// 鍵未生成時のパニック確認
    #[test]
    #[should_panic(expected = "署名用キーペアが未生成です")]
    fn test_sign_without_keypair_panics() {
        let rt = MockRuntime::new();
        rt.sign(b"test");
    }

    /// 暗号化鍵未生成時のパニック確認
    #[test]
    #[should_panic(expected = "暗号化用キーペアが未生成です")]
    fn test_encryption_pubkey_without_keypair_panics() {
        let rt = MockRuntime::new();
        rt.encryption_pubkey();
    }
}
