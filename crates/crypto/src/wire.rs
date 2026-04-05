// SPDX-License-Identifier: Apache-2.0

//! # ワイヤーフォーマット
//!
//! 暗号化ペイロードのバイナリフォーマット解析・構築。
//!
//! ## フォーマット v1
//! ```text
//! ┌──────────┬───────────────┬────────────┬───────────┬────────────┐
//! │ suite_id │ encap_key_len │ encap_key  │   nonce   │ ciphertext │
//! │  (1 B)   │   (2 B, BE)   │ (variable) │ (variable)│  (残り)    │
//! └──────────┴───────────────┴────────────┴───────────┴────────────┘
//! ```
//!
//! - `suite_id`: `CryptoSuite` の `id()` バイト
//! - `encap_key_len`: encapsulated_keyの長さ（big-endian u16）
//! - `nonce`長は `CryptoSuite::nonce_size()` で決定
//! - `ciphertext`: 残り全て（AEADタグを含む）

use crate::algorithm::CryptoSuite;
use crate::error::CryptoError;

/// パース済みペイロード。
pub struct ParsedPayload<'a> {
    pub suite: CryptoSuite,
    pub encap_key: &'a [u8],
    pub nonce: &'a [u8],
    pub ciphertext: &'a [u8],
}

/// ワイヤーフォーマットを解析する。
pub fn parse_wire(payload: &[u8]) -> Result<ParsedPayload<'_>, CryptoError> {
    // 最小: suite_id(1) + encap_key_len(2) + nonce(>=0) + ciphertext(>=1)
    if payload.len() < 4 {
        return Err(CryptoError::InvalidWireFormat(
            "payload too short".to_string(),
        ));
    }

    let suite_id = payload[0];
    let suite = CryptoSuite::from_id(suite_id)
        .ok_or(CryptoError::UnsupportedSuite(suite_id))?;

    let encap_key_len = u16::from_be_bytes([payload[1], payload[2]]) as usize;
    let nonce_size = suite.nonce_size();

    let header_len = 3 + encap_key_len + nonce_size;
    if payload.len() < header_len + 1 {
        return Err(CryptoError::InvalidWireFormat(format!(
            "payload too short: need at least {} bytes, got {}",
            header_len + 1,
            payload.len()
        )));
    }

    let encap_key = &payload[3..3 + encap_key_len];
    let nonce = &payload[3 + encap_key_len..3 + encap_key_len + nonce_size];
    let ciphertext = &payload[header_len..];

    Ok(ParsedPayload {
        suite,
        encap_key,
        nonce,
        ciphertext,
    })
}

/// ワイヤーフォーマットを構築する。
pub fn build_wire(
    suite: CryptoSuite,
    encap_key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Vec<u8> {
    assert!(
        encap_key.len() <= u16::MAX as usize,
        "encap_key exceeds u16 max length: {}",
        encap_key.len()
    );
    let encap_key_len = encap_key.len() as u16;
    let total = 3 + encap_key.len() + nonce.len() + ciphertext.len();
    let mut out = Vec::with_capacity(total);
    out.push(suite.id());
    out.extend_from_slice(&encap_key_len.to_be_bytes());
    out.extend_from_slice(encap_key);
    out.extend_from_slice(nonce);
    out.extend_from_slice(ciphertext);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let suite = CryptoSuite::X25519Aes256Gcm;
        let encap_key = [0xAA; 32];
        let nonce = [0xBB; 12];
        let ciphertext = b"encrypted data here";

        let wire = build_wire(suite, &encap_key, &nonce, ciphertext);
        let parsed = parse_wire(&wire).unwrap();

        assert_eq!(parsed.suite, suite);
        assert_eq!(parsed.encap_key, &encap_key);
        assert_eq!(parsed.nonce, &nonce);
        assert_eq!(parsed.ciphertext, ciphertext);
    }

    #[test]
    fn too_short_fails() {
        assert!(parse_wire(&[0x01, 0x00]).is_err());
    }

    #[test]
    fn unknown_suite_fails() {
        assert!(parse_wire(&[0xFF, 0x00, 0x01, 0xAA, 0x00]).is_err());
    }
}
