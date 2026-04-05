// SPDX-License-Identifier: Apache-2.0

//! # 暗号化チャネル合成関数
//!
//! 仕様書 §6.4
//!
//! KEM + KDF(HKDF-SHA256) + AEAD を合成し、`open_request` / `seal_for` を提供する。
//! shared_secretはこのモジュール内で完結し、外部に露出しない。
//!
//! 方向別鍵導出: TLS 1.3 (RFC 8446) と同様、request方向とresponse方向で
//! 異なるHKDF infoから独立した鍵を導出する（RFC 5869 §3.2）。

use std::sync::atomic::{AtomicU64, Ordering};

use hkdf::Hkdf;
use sha2::Sha256;

use crate::algorithm::CryptoSuite;
use crate::error::CryptoError;
use crate::impls::aes256gcm::Aes256GcmAead;
use crate::kem::{Decapsulator, Encapsulator};
use crate::wire;

/// `open_request` の戻り値。
pub struct OpenedRequest {
    /// 復号された平文。
    pub plaintext: Vec<u8>,
    /// レスポンス暗号化用チャネル。
    pub channel: ResponseSealer,
}

/// レスポンス暗号化構造体。AEAD鍵とnonce管理を内部に保持。
pub struct ResponseSealer {
    aead: Aes256GcmAead,
    nonce_counter: AtomicU64,
    nonce_size: usize,
}

/// nonce counterの上限。AES-GCMの安全限界 (2^32) よりはるかに低い値で十分。
const MAX_NONCE_COUNT: u64 = 1_000_000;

impl ResponseSealer {
    /// レスポンスを暗号化する。nonce生成は内部で行う。
    ///
    /// 戻り値: `nonce || ciphertext`（ワイヤーフォーマット済み）
    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.next_nonce()?;
        let ct = self.aead.encrypt(&nonce, aad, plaintext)?;
        let mut out = Vec::with_capacity(nonce.len() + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    fn next_nonce(&self) -> Result<Vec<u8>, CryptoError> {
        let count = self.nonce_counter.fetch_add(1, Ordering::SeqCst);
        if count >= MAX_NONCE_COUNT {
            return Err(CryptoError::NonceExhausted);
        }
        // nonce構造: [0x01 (response direction)][zeros(3)][counter (8B, BE)]
        let mut nonce = vec![0u8; self.nonce_size];
        nonce[0] = 0x01; // response direction
        if self.nonce_size >= 12 {
            nonce[4..12].copy_from_slice(&count.to_be_bytes());
        }
        Ok(nonce)
    }
}

// ---------------------------------------------------------------------------
// 内部ヘルパー: HKDF鍵導出
// ---------------------------------------------------------------------------

/// 方向別に独立した鍵を導出する。
/// RFC 5869 §3.2: 同一IKMから別infoで独立鍵導出。
fn derive_keys(shared_secret: &[u8], encap_key: &[u8]) -> Result<([u8; 32], [u8; 32]), CryptoError> {
    // Extract: salt = encap_key, IKM = shared_secret
    let hkdf = Hkdf::<Sha256>::new(Some(encap_key), shared_secret);

    let mut request_key = [0u8; 32];
    hkdf.expand(b"title-request-key", &mut request_key)
        .map_err(|e| CryptoError::HkdfError(e.to_string()))?;

    let mut response_key = [0u8; 32];
    hkdf.expand(b"title-response-key", &mut response_key)
        .map_err(|e| CryptoError::HkdfError(e.to_string()))?;

    Ok((request_key, response_key))
}

// ---------------------------------------------------------------------------
// 合成関数
// ---------------------------------------------------------------------------

/// TEE側: 暗号化ペイロードを復号する。
///
/// KEM, KDF, nonce, ワイヤーフォーマット解析を全て内部で行う。
/// `shared_secret` はこの関数スコープ内で消滅する。
///
/// 仕様書 §6.4.4: "暗号化ペイロードを復号"
pub fn open_request(
    decapsulator: &dyn Decapsulator,
    wire_payload: &[u8],
    aad: &[u8],
) -> Result<OpenedRequest, CryptoError> {
    let parsed = wire::parse_wire(wire_payload)?;

    // KEM: decapsulate
    let shared_secret = decapsulator.decapsulate(parsed.encap_key)?;

    // KDF: 方向別鍵導出
    let (request_key, response_key) = derive_keys(&shared_secret, parsed.encap_key)?;
    // shared_secret はここで最後。以後使用しない。
    drop(shared_secret);

    // AEAD: 復号
    let request_aead =
        Aes256GcmAead::from_key(&request_key)?;
    let plaintext = request_aead.decrypt(parsed.nonce, aad, parsed.ciphertext)?;

    // ResponseSealer構築
    let response_aead =
        Aes256GcmAead::from_key(&response_key)?;
    let channel = ResponseSealer {
        aead: response_aead,
        nonce_counter: AtomicU64::new(0),
        nonce_size: parsed.suite.nonce_size(),
    };

    Ok(OpenedRequest { plaintext, channel })
}

/// Client側/テスト: 平文を暗号化する。
///
/// 仕様書 §6.4.3: クライアント側暗号化フロー
pub fn seal_for(
    encapsulator: &dyn Encapsulator,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, ResponseSealer), CryptoError> {
    // KEM: encapsulate
    let (shared_secret, encap_key) = encapsulator.encapsulate()?;

    // KDF: 方向別鍵導出
    let (request_key, response_key) = derive_keys(&shared_secret, &encap_key)?;
    drop(shared_secret);

    // AEAD: 暗号化
    let request_aead = Aes256GcmAead::from_key(&request_key)?;
    let nonce_size = request_aead.nonce_size();

    // request方向nonce: [0x00 (request direction)][zeros(3)][zeros(8)]
    let mut nonce = vec![0u8; nonce_size];
    nonce[0] = 0x00; // request direction
    let ct = request_aead.encrypt(&nonce, aad, plaintext)?;

    // ワイヤーフォーマット構築
    // 現在はX25519+AES-256-GCMのみ
    let suite = CryptoSuite::X25519Aes256Gcm;
    let wire = wire::build_wire(suite, &encap_key, &nonce, &ct);

    // ResponseSealer（レスポンス復号用）
    let response_aead = Aes256GcmAead::from_key(&response_key)?;
    let channel = ResponseSealer {
        aead: response_aead,
        nonce_counter: AtomicU64::new(0),
        nonce_size,
    };

    Ok((wire, channel))
}

use crate::aead::Aead as AeadTrait;

impl ResponseSealer {
    /// レスポンスを復号する（クライアント側で使用）。
    ///
    /// 入力: `nonce || ciphertext`
    pub fn open(&self, nonce_and_ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if nonce_and_ciphertext.len() < self.nonce_size + 1 {
            return Err(CryptoError::InvalidWireFormat(
                "response too short".to_string(),
            ));
        }
        let nonce = &nonce_and_ciphertext[..self.nonce_size];
        let ciphertext = &nonce_and_ciphertext[self.nonce_size..];
        self.aead.decrypt(nonce, aad, ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impls::x25519::{X25519Decapsulator, X25519Encapsulator};
    use crate::kem::Decapsulator;

    #[test]
    fn seal_open_roundtrip() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(seed);
        let pk_arr: [u8; 32] = decap.public_key_bytes().try_into().unwrap();
        let encap = X25519Encapsulator::from_public_key(pk_arr);

        let plaintext = b"hello title protocol";
        let aad = b"/verify";

        // Client: seal
        let (wire, client_channel) = seal_for(&encap, plaintext, aad).unwrap();

        // TEE: open
        let opened = open_request(&decap, &wire, aad).unwrap();
        assert_eq!(opened.plaintext, plaintext);

        // TEE: seal response
        let response = b"signed json here";
        let resp_wire = opened.channel.seal(response, aad).unwrap();

        // Client: open response
        let resp_plain = client_channel.open(&resp_wire, aad).unwrap();
        assert_eq!(resp_plain, response);
    }

    #[test]
    fn wrong_aad_fails() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(seed);
        let pk_arr: [u8; 32] = decap.public_key_bytes().try_into().unwrap();
        let encap = X25519Encapsulator::from_public_key(pk_arr);

        let (wire, _) = seal_for(&encap, b"data", b"/verify").unwrap();

        // Wrong AAD on open
        assert!(open_request(&decap, &wire, b"/sign").is_err());
    }

    #[test]
    fn different_sessions_produce_different_keys() {
        let seed: [u8; 32] = rand::random();
        let decap = X25519Decapsulator::from_seed(seed);
        let pk_arr: [u8; 32] = decap.public_key_bytes().try_into().unwrap();
        let encap = X25519Encapsulator::from_public_key(pk_arr);

        let (wire1, _) = seal_for(&encap, b"data", &[]).unwrap();
        let (wire2, _) = seal_for(&encap, b"data", &[]).unwrap();

        // Different encap keys → different wire payloads
        assert_ne!(wire1, wire2);
    }
}
