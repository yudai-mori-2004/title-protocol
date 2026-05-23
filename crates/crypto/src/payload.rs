// SPDX-License-Identifier: Apache-2.0

//! # Encrypted Payload Internal Structure
//!
//! Spec §2.4
//!
//! Plaintext payload before encryption:
//! ```text
//! [4B: metadata_len (big-endian u32)]
//! [metadata_len bytes: JSON EncryptedPayloadMetadata]
//! [remaining: raw content bytes]
//! ```

use title_core::EncryptedPayloadMetadata;

use crate::CryptoError;

/// Build a plaintext payload from metadata and content.
/// Spec §2.4
pub fn build_payload(metadata: &EncryptedPayloadMetadata, content: &[u8]) -> Vec<u8> {
    let meta_json = serde_json::to_vec(metadata).expect("metadata serialization cannot fail");
    let meta_len = meta_json.len() as u32;

    let mut buf = Vec::with_capacity(4 + meta_json.len() + content.len());
    buf.extend_from_slice(&meta_len.to_be_bytes());
    buf.extend_from_slice(&meta_json);
    buf.extend_from_slice(content);
    buf
}

/// Parsed payload.
pub struct ParsedPayload<'a> {
    pub metadata: EncryptedPayloadMetadata,
    pub content: &'a [u8],
}

/// Parse a plaintext payload into metadata and content.
/// Spec §2.4
pub fn parse_payload(data: &[u8]) -> Result<ParsedPayload<'_>, CryptoError> {
    if data.len() < 4 {
        return Err(CryptoError::InvalidPayload(
            "payload too short for metadata_len".into(),
        ));
    }

    let meta_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let meta_end = 4 + meta_len;

    if data.len() < meta_end {
        return Err(CryptoError::InvalidPayload(format!(
            "payload too short: need {} bytes for metadata, have {}",
            meta_end,
            data.len()
        )));
    }

    let metadata: EncryptedPayloadMetadata =
        serde_json::from_slice(&data[4..meta_end]).map_err(|e| {
            CryptoError::InvalidPayload(format!("metadata JSON parse failed: {e}"))
        })?;

    Ok(ParsedPayload {
        metadata,
        content: &data[meta_end..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let meta = EncryptedPayloadMetadata {
            signature_hash: "sha256:abc123".into(),
        };
        let content = b"raw jpeg bytes here";

        let payload = build_payload(&meta, content);
        let parsed = parse_payload(&payload).unwrap();

        assert_eq!(parsed.metadata.signature_hash, "sha256:abc123");
        assert_eq!(parsed.content, content);
    }

    #[test]
    fn empty_content() {
        let meta = EncryptedPayloadMetadata {
            signature_hash: "sha256:empty".into(),
        };
        let payload = build_payload(&meta, &[]);
        let parsed = parse_payload(&payload).unwrap();

        assert_eq!(parsed.metadata.signature_hash, "sha256:empty");
        assert!(parsed.content.is_empty());
    }

    #[test]
    fn truncated_payload() {
        assert!(parse_payload(&[0, 0, 0]).is_err());
        assert!(parse_payload(&[0, 0, 0, 10]).is_err());
    }
}
