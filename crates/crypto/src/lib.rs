// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol 暗号処理
//!
//! 仕様書セクション1.1およびセクション6.4で定義されるハイブリッド暗号化仕様を実装する。
//!
//! ## 暗号アルゴリズム
//! | 用途 | アルゴリズム |
//! |------|------------|
//! | 鍵交換 | X25519 ECDH |
//! | 鍵導出 | HKDF-SHA256 |
//! | 対称暗号 | AES-256-GCM |
//! | 署名 | Ed25519 |
//! | ハッシュ | SHA-256 |
//!
//! ## Attestation Document検証
//! `attestation` モジュールでVM型TEEのAttestation Document検証を提供する。
//! ベンダー実装はfeature flagで分離される。

pub mod attestation;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use ed25519_dalek::{Signer, Verifier};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

pub use ed25519_dalek::{
    SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey,
    Signature as Ed25519Signature,
};

/// 暗号処理のエラー型
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// ECDH鍵交換エラー
    #[error("ECDH鍵交換に失敗しました")]
    EcdhError,
    /// HKDF鍵導出エラー
    #[error("HKDF鍵導出に失敗しました: {0}")]
    HkdfError(String),
    /// AES-GCM暗号化エラー
    #[error("AES-GCM暗号化に失敗しました")]
    EncryptError,
    /// AES-GCM復号エラー
    #[error("AES-GCM復号に失敗しました")]
    DecryptError,
    /// Ed25519署名検証エラー
    #[error("Ed25519署名検証に失敗しました")]
    SignatureVerifyError,
}

/// 対称鍵（AES-256用、32バイト）
pub type SymmetricKey = [u8; 32];

/// X25519 ECDHによる共有秘密の導出。
/// 仕様書 §6.4 ハイブリッド暗号化 Step 3
///
/// クライアント側: `ECDH(eph_sk, tee_pk)`
/// TEE側: `ECDH(tee_sk, eph_pk)`
pub fn ecdh_derive_shared_secret(
    secret_key: &X25519StaticSecret,
    public_key: &X25519PublicKey,
) -> [u8; 32] {
    let shared = secret_key.diffie_hellman(public_key);
    *shared.as_bytes()
}

/// HKDF-SHA256による対称鍵の導出。
/// 仕様書 §6.4 ハイブリッド暗号化 Step 4
pub fn hkdf_derive_key(shared_secret: &[u8; 32]) -> Result<SymmetricKey, CryptoError> {
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut key = [0u8; 32];
    hkdf.expand(b"title-protocol-e2ee", &mut key)
        .map_err(|e| CryptoError::HkdfError(e.to_string()))?;
    Ok(key)
}

/// AES-256-GCMによる暗号化。
/// 仕様書 §6.4 ハイブリッド暗号化 Step 4
pub fn aes_gcm_encrypt(
    key: &SymmetricKey,
    nonce: &[u8; 12],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::EncryptError)?;
    let nonce = Nonce::from(*nonce);
    cipher.encrypt(&nonce, plaintext).map_err(|_| CryptoError::EncryptError)
}

/// AES-256-GCMによる復号。
/// 仕様書 §6.4 ハイブリッド暗号化 Step 7
pub fn aes_gcm_decrypt(
    key: &SymmetricKey,
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::DecryptError)?;
    let nonce = Nonce::from(*nonce);
    cipher.decrypt(&nonce, ciphertext).map_err(|_| CryptoError::DecryptError)
}

/// Ed25519による署名。
/// 仕様書 §5.1 Step 4 (tee_signature)
pub fn ed25519_sign(signing_key: &Ed25519SigningKey, message: &[u8]) -> Ed25519Signature {
    signing_key.sign(message)
}

/// Ed25519による署名検証。
/// 仕様書 §5.2 Step 4
pub fn ed25519_verify(
    verifying_key: &Ed25519VerifyingKey,
    message: &[u8],
    signature: &Ed25519Signature,
) -> Result<(), CryptoError> {
    verifying_key
        .verify(message, signature)
        .map_err(|_| CryptoError::SignatureVerifyError)
}

/// SHA-256ハッシュ計算。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Active Manifestの署名からcontent_hashを計算する。
/// 仕様書 §2.1: `content_hash = SHA-256(Active Manifestの署名)`
pub fn content_hash_from_manifest_signature(manifest_signature: &[u8]) -> [u8; 32] {
    sha256(manifest_signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ECDH + HKDF + AES-GCM roundtrip（プロトコル §6.4 の暗号化フロー全体）
    // -----------------------------------------------------------------------

    #[test]
    fn test_ecdh_hkdf_aes_gcm_roundtrip() {
        // クライアント側: エフェメラル鍵ペア生成
        let client_secret = X25519StaticSecret::random_from_rng(rand::rngs::OsRng);
        let client_pubkey = X25519PublicKey::from(&client_secret);

        // TEE側: 鍵ペア生成
        let tee_secret = X25519StaticSecret::random_from_rng(rand::rngs::OsRng);
        let tee_pubkey = X25519PublicKey::from(&tee_secret);

        // 双方が同一の共有秘密を導出できること
        let client_shared = ecdh_derive_shared_secret(&client_secret, &tee_pubkey);
        let tee_shared = ecdh_derive_shared_secret(&tee_secret, &client_pubkey);
        assert_eq!(client_shared, tee_shared);

        // HKDF で対称鍵を導出
        let client_key = hkdf_derive_key(&client_shared).unwrap();
        let tee_key = hkdf_derive_key(&tee_shared).unwrap();
        assert_eq!(client_key, tee_key);

        // AES-GCM roundtrip
        let nonce = [0u8; 12];
        let plaintext = b"hello title protocol";
        let ciphertext = aes_gcm_encrypt(&client_key, &nonce, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext.to_vec());
        let decrypted = aes_gcm_decrypt(&tee_key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    // -----------------------------------------------------------------------
    // AES-GCM エラーケース
    // -----------------------------------------------------------------------

    #[test]
    fn test_aes_gcm_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let nonce = [0u8; 12];
        let ciphertext = aes_gcm_encrypt(&key1, &nonce, b"secret").unwrap();
        assert!(aes_gcm_decrypt(&key2, &nonce, &ciphertext).is_err());
    }

    #[test]
    fn test_aes_gcm_wrong_nonce_fails() {
        let key = [1u8; 32];
        let nonce1 = [0u8; 12];
        let nonce2 = [1u8; 12];
        let ciphertext = aes_gcm_encrypt(&key, &nonce1, b"secret").unwrap();
        assert!(aes_gcm_decrypt(&key, &nonce2, &ciphertext).is_err());
    }

    #[test]
    fn test_aes_gcm_tampered_ciphertext_fails() {
        let key = [1u8; 32];
        let nonce = [0u8; 12];
        let mut ciphertext = aes_gcm_encrypt(&key, &nonce, b"secret").unwrap();
        ciphertext[0] ^= 0xff;
        assert!(aes_gcm_decrypt(&key, &nonce, &ciphertext).is_err());
    }

    // -----------------------------------------------------------------------
    // Ed25519 署名/検証
    // -----------------------------------------------------------------------

    #[test]
    fn test_ed25519_sign_verify_roundtrip() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = signing_key.verifying_key();
        let message = b"title protocol attestation";

        let signature = ed25519_sign(&signing_key, message);
        assert!(ed25519_verify(&verifying_key, message, &signature).is_ok());
    }

    #[test]
    fn test_ed25519_wrong_message_fails() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = signing_key.verifying_key();

        let signature = ed25519_sign(&signing_key, b"original");
        assert!(ed25519_verify(&verifying_key, b"tampered", &signature).is_err());
    }

    #[test]
    fn test_ed25519_wrong_key_fails() {
        let key1 = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let key2 = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let message = b"test message";

        let signature = ed25519_sign(&key1, message);
        assert!(ed25519_verify(&key2.verifying_key(), message, &signature).is_err());
    }

    // -----------------------------------------------------------------------
    // SHA-256
    // -----------------------------------------------------------------------

    #[test]
    fn test_sha256_known_value() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256(b"");
        assert_eq!(
            hex::encode(hash),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_deterministic() {
        let data = b"title protocol content";
        assert_eq!(sha256(data), sha256(data));
    }

    // -----------------------------------------------------------------------
    // content_hash_from_manifest_signature
    // -----------------------------------------------------------------------

    #[test]
    fn test_content_hash_is_sha256_of_signature() {
        let signature = b"mock cose signature bytes";
        assert_eq!(content_hash_from_manifest_signature(signature), sha256(signature));
    }
}
