// SPDX-License-Identifier: Apache-2.0

//! # Direction-Separated Key Derivation
//!
//! Spec §2.4
//!
//! HKDF-SHA256 derives two independent 32-byte keys from a shared secret:
//! - request_key  (info = "title-request-key",  salt = encap_key)
//! - response_key (info = "title-response-key", salt = encap_key)
//!
//! ## Why `salt = encap_key`
//!
//! Folding `encap_key` into the salt makes the derived keys depend on the
//! ephemeral public material on the wire, not just the shared secret. The
//! AEAD AAD only covers the constant-size suite header (suite_id +
//! encap_key_len); the much larger `encap_key` bytes ride into the key
//! schedule through this salt. A tampered `encap_key` therefore breaks
//! decryption the same way a wrong AAD would, without forcing the AEAD
//! to authenticate kilobytes of header (ML-KEM-768 encap_key = 1088 B).

use hkdf::Hkdf;
use sha2::Sha256;

use crate::CryptoError;

const REQUEST_INFO: &[u8] = b"title-request-key";
const RESPONSE_INFO: &[u8] = b"title-response-key";

/// Derive direction-separated keys from a shared secret.
/// Spec §2.4
///
/// Returns (request_key, response_key), each 32 bytes.
pub fn derive_keys(
    shared_secret: &[u8],
    encap_key: &[u8],
) -> Result<([u8; 32], [u8; 32]), CryptoError> {
    let hkdf = Hkdf::<Sha256>::new(Some(encap_key), shared_secret);

    let mut request_key = [0u8; 32];
    hkdf.expand(REQUEST_INFO, &mut request_key)
        .map_err(|e| CryptoError::HkdfError(e.to_string()))?;

    let mut response_key = [0u8; 32];
    hkdf.expand(RESPONSE_INFO, &mut response_key)
        .map_err(|e| CryptoError::HkdfError(e.to_string()))?;

    Ok((request_key, response_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_key_differs_from_response_key() {
        let shared_secret = [0xAB; 32];
        let encap_key = [0xCD; 32];
        let (req, resp) = derive_keys(&shared_secret, &encap_key).unwrap();
        assert_ne!(req, resp);
    }

    #[test]
    fn deterministic() {
        let shared_secret = [0x01; 32];
        let encap_key = [0x02; 32];
        let (req1, resp1) = derive_keys(&shared_secret, &encap_key).unwrap();
        let (req2, resp2) = derive_keys(&shared_secret, &encap_key).unwrap();
        assert_eq!(req1, req2);
        assert_eq!(resp1, resp2);
    }

    #[test]
    fn different_encap_key_gives_different_keys() {
        let shared_secret = [0x01; 32];
        let (req1, _) = derive_keys(&shared_secret, &[0x02; 32]).unwrap();
        let (req2, _) = derive_keys(&shared_secret, &[0x03; 32]).unwrap();
        assert_ne!(req1, req2);
    }
}
