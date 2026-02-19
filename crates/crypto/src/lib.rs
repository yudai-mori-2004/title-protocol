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
    let nonce = Nonce::from_slice(nonce);
    cipher.encrypt(nonce, plaintext).map_err(|_| CryptoError::EncryptError)
}

/// AES-256-GCMによる復号。
/// 仕様書 §6.4 ハイブリッド暗号化 Step 7
pub fn aes_gcm_decrypt(
    key: &SymmetricKey,
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::DecryptError)?;
    let nonce = Nonce::from_slice(nonce);
    cipher.decrypt(nonce, ciphertext).map_err(|_| CryptoError::DecryptError)
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
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Active Manifestの署名からcontent_hashを計算する。
/// 仕様書 §2.1: `content_hash = SHA-256(Active Manifestの署名)`
pub fn content_hash_from_manifest_signature(manifest_signature: &[u8]) -> [u8; 32] {
    sha256(manifest_signature)
}
