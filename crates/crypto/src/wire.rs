// SPDX-License-Identifier: Apache-2.0

//! # Binary Wire Format
//!
//! Spec §2.4
//!
//! ## Request wire format
//! ```text
//! [suite_id(1B)][encap_key_len(2B BE)][encap_key][nonce(12B)][ciphertext]
//! ```
//!
//! ## Response wire format
//! ```text
//! [nonce(12B)][ciphertext]
//! ```

use title_core::EncryptionSuite;

use crate::aead::NONCE_SIZE;
use crate::kem::encap_key_len;
use crate::CryptoError;

// ---------------------------------------------------------------------------
// Request wire format
// ---------------------------------------------------------------------------

/// Parsed request wire format.
/// Spec §2.4
pub struct ParsedRequest<'a> {
    pub suite: EncryptionSuite,
    pub encap_key: &'a [u8],
    pub nonce: &'a [u8],
    pub ciphertext: &'a [u8],
}

/// Parse request wire format.
/// Spec §2.4
pub fn parse_request(payload: &[u8]) -> Result<ParsedRequest<'_>, CryptoError> {
    if payload.len() < 3 {
        return Err(CryptoError::InvalidWireFormat(
            "payload too short for header".into(),
        ));
    }

    let suite_id = payload[0];
    let suite =
        EncryptionSuite::from_suite_id(suite_id).ok_or(CryptoError::UnsupportedSuite(suite_id))?;

    let ek_len = u16::from_be_bytes([payload[1], payload[2]]) as usize;
    let expected_ek_len = encap_key_len(suite);
    if ek_len != expected_ek_len {
        return Err(CryptoError::InvalidWireFormat(format!(
            "encap_key_len mismatch: header says {ek_len}, suite expects {expected_ek_len}"
        )));
    }

    let ek_end = 3 + ek_len;
    let nonce_end = ek_end + NONCE_SIZE;

    if payload.len() < nonce_end {
        return Err(CryptoError::InvalidWireFormat(
            "payload too short for encap_key + nonce".into(),
        ));
    }

    Ok(ParsedRequest {
        suite,
        encap_key: &payload[3..ek_end],
        nonce: &payload[ek_end..nonce_end],
        ciphertext: &payload[nonce_end..],
    })
}

/// Build request wire format.
/// Spec §2.4
pub fn build_request(
    suite: EncryptionSuite,
    encap_key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Vec<u8> {
    let ek_len = encap_key.len() as u16;
    let mut buf = Vec::with_capacity(1 + 2 + encap_key.len() + nonce.len() + ciphertext.len());
    buf.push(suite.suite_id());
    buf.extend_from_slice(&ek_len.to_be_bytes());
    buf.extend_from_slice(encap_key);
    buf.extend_from_slice(nonce);
    buf.extend_from_slice(ciphertext);
    buf
}

// ---------------------------------------------------------------------------
// Response wire format
// ---------------------------------------------------------------------------

/// Parsed response wire format.
/// Spec §2.4
pub struct ParsedResponse<'a> {
    pub nonce: &'a [u8],
    pub ciphertext: &'a [u8],
}

/// Parse response wire format.
/// Spec §2.4
pub fn parse_response(payload: &[u8]) -> Result<ParsedResponse<'_>, CryptoError> {
    if payload.len() < NONCE_SIZE {
        return Err(CryptoError::InvalidWireFormat(
            "response payload too short for nonce".into(),
        ));
    }
    Ok(ParsedResponse {
        nonce: &payload[..NONCE_SIZE],
        ciphertext: &payload[NONCE_SIZE..],
    })
}

/// Build response wire format.
/// Spec §2.4
pub fn build_response(nonce: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(nonce.len() + ciphertext.len());
    buf.extend_from_slice(nonce);
    buf.extend_from_slice(ciphertext);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_x25519() {
        let suite = EncryptionSuite::X25519;
        let encap_key = [0xAA; 32];
        let nonce = [0xBB; 12];
        let ciphertext = b"encrypted data here";

        let wire = build_request(suite, &encap_key, &nonce, ciphertext);
        let parsed = parse_request(&wire).unwrap();

        assert_eq!(parsed.suite, suite);
        assert_eq!(parsed.encap_key, &encap_key);
        assert_eq!(parsed.nonce, &nonce);
        assert_eq!(parsed.ciphertext, ciphertext);
    }

    #[test]
    fn request_roundtrip_mlkem() {
        let suite = EncryptionSuite::MlKem768;
        let encap_key = [0xCC; 1088];
        let nonce = [0xDD; 12];
        let ciphertext = b"post-quantum ciphertext";

        let wire = build_request(suite, &encap_key, &nonce, ciphertext);
        let parsed = parse_request(&wire).unwrap();

        assert_eq!(parsed.suite, suite);
        assert_eq!(parsed.encap_key.len(), 1088);
        assert_eq!(parsed.ciphertext, ciphertext);
    }

    #[test]
    fn response_roundtrip() {
        let nonce = [0xEE; 12];
        let ciphertext = b"response ciphertext";

        let wire = build_response(&nonce, ciphertext);
        let parsed = parse_response(&wire).unwrap();

        assert_eq!(parsed.nonce, &nonce);
        assert_eq!(parsed.ciphertext, ciphertext);
    }

    #[test]
    fn invalid_suite_id() {
        let wire = [0xFF, 0x00, 0x20];
        assert!(matches!(
            parse_request(&wire),
            Err(CryptoError::UnsupportedSuite(0xFF))
        ));
    }

    #[test]
    fn truncated_request() {
        let wire = [0x01, 0x00];
        assert!(parse_request(&wire).is_err());
    }

    #[test]
    fn truncated_response() {
        let wire = [0x00; 5];
        assert!(parse_response(&wire).is_err());
    }
}
