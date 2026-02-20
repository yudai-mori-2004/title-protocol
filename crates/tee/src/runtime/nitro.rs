//! # AWS Nitro Enclaves ランタイム実装
//!
//! 仕様書 §6.4
//!
//! AWS Nitro Enclaves上で動作するTEEランタイム。
//! NSM (Nitro Security Module) APIを使用して鍵生成とAttestation取得を行う。
//!
//! ## 設計
//!
//! NSMデバイス操作は `NsmOps` トレイトで抽象化し、テスト時にはモック注入が可能。
//! - 本番（Linux/Nitro Enclave）: `RealNsm` — `/dev/nsm` 経由でNSM APIを呼び出し
//! - テスト: `MockNsm` — `OsRng` でエントロピー生成、モックAttestation返却

use std::sync::RwLock;

use ed25519_dalek::{Signer, SigningKey};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use super::TeeRuntime;

// ─────────────────────────────────────────────
// NSMデバイス操作の抽象化
// ─────────────────────────────────────────────

/// NSMデバイス操作の抽象化トレイト。
/// 仕様書 §6.4
///
/// テスト時にはモック実装を注入することで、
/// NSMハードウェアなしでNitroRuntimeをテスト可能にする。
trait NsmOps: Send + Sync {
    /// NSMデバイスからランダムバイトを取得する。
    fn get_random(&self, len: usize) -> Vec<u8>;

    /// Attestation Documentを取得する。
    ///
    /// - `public_key`: Attestation Documentに含めるTEE署名用公開鍵
    /// - `user_data`: Attestation Documentに含める追加データ（暗号化用公開鍵）
    /// - `nonce`: フレッシュネス用ノンス
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
    use aws_nitro_enclaves_nsm_api::driver as nsm_driver;
    use serde_bytes::ByteBuf;

    /// 本番用NSMデバイス。
    /// `/dev/nsm` を開き、NSM APIを呼び出す。
    pub struct RealNsm {
        fd: i32,
    }

    impl RealNsm {
        /// NSMデバイスを初期化する。
        /// Nitro Enclave内でのみ動作する。
        pub fn new() -> Self {
            let fd = nsm_driver::nsm_init();
            assert!(
                fd >= 0,
                "NSMデバイスの初期化に失敗（Nitro Enclave外で実行していませんか？）"
            );
            Self { fd }
        }
    }

    impl Drop for RealNsm {
        fn drop(&mut self) {
            nsm_driver::nsm_exit(self.fd);
        }
    }

    impl NsmOps for RealNsm {
        /// NSM APIの `GetRandom` リクエストでランダムバイトを取得する。
        /// 仕様書 §6.4 — NSMエントロピーによる鍵生成
        fn get_random(&self, len: usize) -> Vec<u8> {
            let mut result = Vec::with_capacity(len);
            while result.len() < len {
                match nsm_driver::nsm_process_request(self.fd, Request::GetRandom) {
                    Response::GetRandom { random } => {
                        result.extend_from_slice(&random);
                    }
                    other => panic!(
                        "NSM GetRandomが予期しないレスポンスを返しました: {:?}",
                        other
                    ),
                }
            }
            result.truncate(len);
            result
        }

        /// NSM APIの `Attestation` リクエストでAttestation Documentを取得する。
        /// 仕様書 §5.2 Step 4.1
        fn get_attestation_doc(
            &self,
            public_key: Option<&[u8]>,
            user_data: Option<&[u8]>,
            nonce: Option<&[u8]>,
        ) -> Vec<u8> {
            let request = Request::Attestation {
                public_key: public_key.map(|k| ByteBuf::from(k.to_vec())),
                user_data: user_data.map(|d| ByteBuf::from(d.to_vec())),
                nonce: nonce.map(|n| ByteBuf::from(n.to_vec())),
            };

            match nsm_driver::nsm_process_request(self.fd, request) {
                Response::Attestation { document } => document,
                other => panic!(
                    "NSM Attestationが予期しないレスポンスを返しました: {:?}",
                    other
                ),
            }
        }
    }
}

// ─────────────────────────────────────────────
// モックNSMデバイス（テスト用）
// ─────────────────────────────────────────────

#[cfg(test)]
mod mock_nsm {
    use super::NsmOps;

    /// テスト用モックNSMデバイス。
    /// `OsRng` でエントロピーを生成し、モックAttestation Documentを返す。
    pub(super) struct MockNsm;

    impl NsmOps for MockNsm {
        /// `OsRng` でランダムバイトを生成する。
        fn get_random(&self, len: usize) -> Vec<u8> {
            use rand::RngCore;
            let mut buf = vec![0u8; len];
            rand::rngs::OsRng.fill_bytes(&mut buf);
            buf
        }

        /// モックAttestation Documentを返す。
        /// 仕様書 §5.2 Step 4.1
        ///
        /// 実際のNitro Attestation Documentと同様のフィールドを持つが、
        /// COSE Sign1ではなくJSON形式のモック。PCR値は全てゼロ。
        fn get_attestation_doc(
            &self,
            public_key: Option<&[u8]>,
            user_data: Option<&[u8]>,
            nonce: Option<&[u8]>,
        ) -> Vec<u8> {
            let doc = serde_json::json!({
                "module_id": "nitro-runtime-mock",
                "digest": "SHA384",
                "timestamp": 1700000000u64,
                "pcrs": {
                    "0": vec![0u8; 48],
                    "1": vec![0u8; 48],
                    "2": vec![0u8; 48],
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
/// NSM (Nitro Security Module) APIを使用して鍵生成とAttestation取得を行う。
/// 全ての秘密鍵はEnclave内メモリにのみ保持され、外部にはエクスポートされない。
/// TEE再起動時は新しいキーペアが生成される（KMSなし）。
pub struct NitroRuntime {
    /// NSMデバイス操作（テスト時はモック注入可能）
    nsm: Box<dyn NsmOps>,
    /// Ed25519署名用キーペア（メモリ内のみ保持）
    signing_key: RwLock<Option<SigningKey>>,
    /// X25519暗号化用秘密鍵（メモリ内のみ保持）
    encryption_secret: RwLock<Option<StaticSecret>>,
    /// Tree用Ed25519キーペア（メモリ内のみ保持）
    tree_key: RwLock<Option<SigningKey>>,
}

impl NitroRuntime {
    /// NitroRuntimeを初期化する（本番用）。
    /// 仕様書 §6.4
    ///
    /// NSMデバイス `/dev/nsm` を開き、ランタイムを初期化する。
    /// Nitro Enclave内でのみ動作する。
    #[cfg(target_os = "linux")]
    pub fn new() -> Self {
        Self {
            nsm: Box::new(real_nsm::RealNsm::new()),
            signing_key: RwLock::new(None),
            encryption_secret: RwLock::new(None),
            tree_key: RwLock::new(None),
        }
    }

    /// NitroRuntimeは非Linux環境では利用不可。
    /// ローカル開発にはMockRuntimeを使用すること。
    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        panic!(
            "NitroRuntimeはLinux (Nitro Enclave) 環境でのみ使用可能です。\
             ローカル開発にはMockRuntimeを使用してください。"
        )
    }

    /// テスト用: モックNSMデバイスでNitroRuntimeを作成する。
    #[cfg(test)]
    pub(crate) fn with_mock() -> Self {
        Self {
            nsm: Box::new(mock_nsm::MockNsm),
            signing_key: RwLock::new(None),
            encryption_secret: RwLock::new(None),
            tree_key: RwLock::new(None),
        }
    }
}

impl TeeRuntime for NitroRuntime {
    /// AWS Nitro EnclaveのTEE種別を返す。
    fn tee_type(&self) -> &str {
        "aws_nitro"
    }

    /// NSM APIのエントロピーでEd25519署名用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    ///
    /// NSMデバイスから32バイトのエントロピーを取得し、
    /// Ed25519の秘密鍵シードとして使用する。
    /// 秘密鍵はEnclave内メモリにのみ保持される。
    fn generate_signing_keypair(&self) {
        let entropy = self.nsm.get_random(32);
        let seed: [u8; 32] = entropy
            .try_into()
            .expect("NSMエントロピーは32バイトであるべき");
        let signing_key = SigningKey::from_bytes(&seed);
        let mut guard = self.signing_key.write().unwrap();
        *guard = Some(signing_key);
    }

    /// NSM APIのエントロピーでX25519暗号化用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    ///
    /// NSMデバイスから32バイトのエントロピーを取得し、
    /// X25519の秘密鍵として使用する。
    /// 秘密鍵はEnclave内メモリにのみ保持される。
    fn generate_encryption_keypair(&self) {
        let entropy = self.nsm.get_random(32);
        let seed: [u8; 32] = entropy
            .try_into()
            .expect("NSMエントロピーは32バイトであるべき");
        let secret = StaticSecret::from(seed);
        let mut guard = self.encryption_secret.write().unwrap();
        *guard = Some(secret);
    }

    /// NSM APIからAttestation Documentを取得する。
    /// 仕様書 §5.2 Step 4.1
    ///
    /// Attestation Documentには以下を含む:
    /// - `public_key`: Ed25519署名用公開鍵
    /// - `user_data`: X25519暗号化用公開鍵
    /// - PCR値（PCR0, PCR1, PCR2）: Enclaveイメージの測定値
    fn get_attestation(&self) -> Vec<u8> {
        let signing_pk = self.signing_pubkey();
        let encryption_pk = self.encryption_pubkey();
        self.nsm.get_attestation_doc(
            Some(&signing_pk),
            Some(&encryption_pk),
            None,
        )
    }

    /// 署名用秘密鍵でデータに署名する。
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

    /// NSM APIのエントロピーでTree用Ed25519キーペアを生成する。
    /// 仕様書 §6.4 Step 2
    ///
    /// Tree用公開鍵がそのままMerkle Treeのアカウントアドレスとなる。
    fn generate_tree_keypair(&self) {
        let entropy = self.nsm.get_random(32);
        let seed: [u8; 32] = entropy
            .try_into()
            .expect("NSMエントロピーは32バイトであるべき");
        let key = SigningKey::from_bytes(&seed);
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
        let rt = NitroRuntime::with_mock();
        rt.generate_signing_keypair();

        let message = b"NitroRuntime test message";
        let sig_bytes = rt.sign(message);
        let pubkey_bytes = rt.signing_pubkey();

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
        let rt = NitroRuntime::with_mock();
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
        let rt = NitroRuntime::with_mock();
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
        let rt = NitroRuntime::with_mock();
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();

        let attestation = rt.get_attestation();
        assert!(!attestation.is_empty());

        // モックAttestation DocumentはJSON形式
        let doc: serde_json::Value =
            serde_json::from_slice(&attestation).expect("有効なJSON");

        assert_eq!(doc["module_id"], "nitro-runtime-mock");

        // 署名用公開鍵が public_key フィールドに含まれる
        let pk: Vec<u8> = serde_json::from_value(doc["public_key"].clone()).unwrap();
        assert_eq!(pk.len(), 32);
        assert_eq!(pk, rt.signing_pubkey());

        // 暗号化用公開鍵が user_data フィールドに含まれる
        let epk: Vec<u8> = serde_json::from_value(doc["user_data"].clone()).unwrap();
        assert_eq!(epk.len(), 32);
        assert_eq!(epk, rt.encryption_pubkey());

        // PCR値が含まれる（モックでは全ゼロ）
        let pcrs = &doc["pcrs"];
        let pcr0: Vec<u8> = serde_json::from_value(pcrs["0"].clone()).unwrap();
        assert_eq!(pcr0.len(), 48);
        assert!(pcr0.iter().all(|&b| b == 0));
    }

    /// 鍵未生成時のパニック確認
    #[test]
    #[should_panic(expected = "署名用キーペアが未生成です")]
    fn test_sign_without_keypair_panics() {
        let rt = NitroRuntime::with_mock();
        rt.sign(b"test");
    }

    /// 暗号化鍵未生成時のパニック確認
    #[test]
    #[should_panic(expected = "暗号化用キーペアが未生成です")]
    fn test_encryption_pubkey_without_keypair_panics() {
        let rt = NitroRuntime::with_mock();
        rt.encryption_pubkey();
    }

    /// Tree用キーペアの生成→署名→検証
    #[test]
    fn test_tree_keypair_sign_verify() {
        let rt = NitroRuntime::with_mock();
        rt.generate_tree_keypair();

        let message = b"Tree test message";
        let sig_bytes = rt.tree_sign(message);
        let pubkey_bytes = rt.tree_pubkey();

        let verifying_key =
            VerifyingKey::from_bytes(&pubkey_bytes.try_into().expect("公開鍵は32バイト"))
                .expect("有効なEd25519公開鍵");
        let signature =
            Signature::from_bytes(&sig_bytes.try_into().expect("署名は64バイト"));

        assert!(verifying_key.verify(message, &signature).is_ok());
    }

    /// TEE種別が "aws_nitro" であることを確認
    #[test]
    fn test_tee_type() {
        let rt = NitroRuntime::with_mock();
        assert_eq!(rt.tee_type(), "aws_nitro");
    }
}
