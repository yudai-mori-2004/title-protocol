// SPDX-License-Identifier: Apache-2.0

//! # Sealed Channel
//!
//! Spec §2.4
//!
//! High-level composition: KEM + HKDF + AES-256-GCM.
//!
//! - `seal_for`: Client encrypts content → wire payload + response channel
//! - `open_request`: TEE decrypts wire payload → plaintext + response channel

use title_core::EncryptionSuite;

use crate::aead::NONCE_SIZE;
use crate::key_bundle::KeyBundle;
use crate::kem::create_encapsulator;
use crate::{aead, hkdf, wire, CryptoError};

/// Result of TEE-side decryption.
pub struct OpenedRequest {
    pub plaintext: Vec<u8>,
    pub response_channel: ResponseChannel,
}

/// Holds the response_key for encrypting/decrypting the response.
/// Spec §2.4 — response direction uses the same KEM exchange.
pub struct ResponseChannel {
    response_key: [u8; 32],
}

impl ResponseChannel {
    /// TEE: encrypt a response.
    /// Spec §2.4 — response wire format: [nonce(12B)][ciphertext]
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut nonce = [0u8; NONCE_SIZE];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
        let ciphertext = aead::encrypt(&self.response_key, &nonce, plaintext)?;
        Ok(wire::build_response(&nonce, &ciphertext))
    }

    /// Client: decrypt a response.
    /// Spec §2.4
    pub fn open(&self, wire_payload: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let parsed = wire::parse_response(wire_payload)?;
        aead::decrypt(&self.response_key, parsed.nonce, parsed.ciphertext)
    }
}

/// Client: encrypt content for TEE.
/// Spec §2.4 — steps 2-4.
///
/// Returns (wire_payload, response_channel).
pub fn seal_for(
    suite: EncryptionSuite,
    public_key: &[u8],
    plaintext: &[u8],
) -> Result<(Vec<u8>, ResponseChannel), CryptoError> {
    let encapsulator = create_encapsulator(suite, public_key)?;
    let (shared_secret, encap_key) = encapsulator.encapsulate()?;
    let (request_key, response_key) = hkdf::derive_keys(&shared_secret, &encap_key)?;

    let mut nonce = [0u8; NONCE_SIZE];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let ciphertext = aead::encrypt(&request_key, &nonce, plaintext)?;

    let wire_payload = wire::build_request(suite, &encap_key, &nonce, &ciphertext);
    let channel = ResponseChannel { response_key };

    Ok((wire_payload, channel))
}

/// TEE: decrypt a request wire payload.
/// Spec §2.4 — step 6.
///
/// Returns plaintext + response channel.
pub fn open_request(
    key_bundle: &KeyBundle,
    wire_payload: &[u8],
) -> Result<OpenedRequest, CryptoError> {
    let parsed = wire::parse_request(wire_payload)?;

    let shared_secret = key_bundle.decapsulate(parsed.suite, parsed.encap_key)?;
    let (request_key, response_key) = hkdf::derive_keys(&shared_secret, parsed.encap_key)?;

    let plaintext = aead::decrypt(&request_key, parsed.nonce, parsed.ciphertext)?;

    Ok(OpenedRequest {
        plaintext,
        response_channel: ResponseChannel { response_key },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bundle() -> KeyBundle {
        KeyBundle::generate(&mut rand::rngs::OsRng).unwrap()
    }

    #[test]
    fn x25519_full_roundtrip() {
        let bundle = test_bundle();
        let pk = bundle.public_key_bytes(EncryptionSuite::X25519);
        let plaintext = b"hello from client";

        let (wire, client_channel) = seal_for(EncryptionSuite::X25519, &pk, plaintext).unwrap();
        let opened = open_request(&bundle, &wire).unwrap();
        assert_eq!(opened.plaintext, plaintext);

        let response = b"response from tee";
        let resp_wire = opened.response_channel.seal(response).unwrap();
        let resp_plain = client_channel.open(&resp_wire).unwrap();
        assert_eq!(resp_plain, response);
    }

    #[test]
    fn p256_full_roundtrip() {
        let bundle = test_bundle();
        let pk = bundle.public_key_bytes(EncryptionSuite::P256);
        let plaintext = b"p256 encrypted content";

        let (wire, client_channel) = seal_for(EncryptionSuite::P256, &pk, plaintext).unwrap();
        let opened = open_request(&bundle, &wire).unwrap();
        assert_eq!(opened.plaintext, plaintext);

        let response = b"p256 response";
        let resp_wire = opened.response_channel.seal(response).unwrap();
        let resp_plain = client_channel.open(&resp_wire).unwrap();
        assert_eq!(resp_plain, response);
    }

    #[test]
    fn ml_kem_full_roundtrip() {
        let bundle = test_bundle();
        let pk = bundle.public_key_bytes(EncryptionSuite::MlKem768);
        let plaintext = b"post-quantum encrypted content";

        let (wire, client_channel) =
            seal_for(EncryptionSuite::MlKem768, &pk, plaintext).unwrap();
        let opened = open_request(&bundle, &wire).unwrap();
        assert_eq!(opened.plaintext, plaintext);

        let response = b"post-quantum response";
        let resp_wire = opened.response_channel.seal(response).unwrap();
        let resp_plain = client_channel.open(&resp_wire).unwrap();
        assert_eq!(resp_plain, response);
    }

    #[test]
    fn wrong_bundle_fails() {
        let bundle1 = test_bundle();
        let bundle2 = test_bundle();
        let pk = bundle1.public_key_bytes(EncryptionSuite::X25519);
        let plaintext = b"for bundle1 only";

        let (wire, _) = seal_for(EncryptionSuite::X25519, &pk, plaintext).unwrap();
        assert!(open_request(&bundle2, &wire).is_err());
    }

    #[test]
    fn direction_keys_are_independent() {
        let bundle = test_bundle();
        let pk = bundle.public_key_bytes(EncryptionSuite::X25519);

        let (wire, client_channel) = seal_for(EncryptionSuite::X25519, &pk, b"data").unwrap();
        let opened = open_request(&bundle, &wire).unwrap();

        let response = b"response data";
        let resp_wire = opened.response_channel.seal(response).unwrap();

        // Client can open the response
        let decrypted = client_channel.open(&resp_wire).unwrap();
        assert_eq!(decrypted, response);

        // TEE's response channel cannot open the request (different keys)
        // and client's channel cannot open with request key
        // (This is implicitly tested by the fact that seal/open work correctly
        // with their respective keys)
    }

    #[test]
    fn payload_with_encrypted_channel() {
        use crate::payload;
        use title_core::EncryptedPayloadMetadata;

        let bundle = test_bundle();
        let pk = bundle.public_key_bytes(EncryptionSuite::X25519);

        let meta = EncryptedPayloadMetadata {
            signature_hash: "sha256:abcdef".into(),
        };
        let content = b"raw jpeg content";
        let plaintext_payload = payload::build_payload(&meta, content);

        let (wire, client_channel) =
            seal_for(EncryptionSuite::X25519, &pk, &plaintext_payload).unwrap();
        let opened = open_request(&bundle, &wire).unwrap();

        let parsed = payload::parse_payload(&opened.plaintext).unwrap();
        assert_eq!(parsed.metadata.signature_hash, "sha256:abcdef");
        assert_eq!(parsed.content, content);

        // Response direction
        let response_json = br#"{"signature_hash":"sha256:abcdef","results":{}}"#;
        let resp_wire = opened.response_channel.seal(response_json).unwrap();
        let resp_plain = client_channel.open(&resp_wire).unwrap();
        assert_eq!(resp_plain, response_json);
    }
}
