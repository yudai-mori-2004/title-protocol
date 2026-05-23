// SPDX-License-Identifier: Apache-2.0

//! # JUMBF (ISO 19566-5) minimal parser
//!
//! Spec §1.3 — signature_hash = SHA-256(Active Manifest's COSE signature)
//!
//! Extracts the COSE signature bytes from a C2PA JUMBF data structure
//! for a specified manifest label. The COSE signature is found in the
//! `c2pa.signature` box (identified by a well-known UUID) inside the
//! manifest's JUMBF superbox.
//!
//! Ported from `legacy/v0.1.0/crates/core/src/jumbf.rs` with error type
//! adapted for v0.1.2 processor framework.

use crate::processor::ProcessorError;
use std::io::{Cursor, Read, Seek, SeekFrom};

/// JUMBF box header size (4-byte size + 4-byte type).
const HEADER_SIZE: u64 = 8;

/// JUMBF superbox type "jumb" (0x6A756D62).
const BOX_TYPE_JUMB: u32 = 0x6A75_6D62;
/// JUMBF description box type "jumd" (0x6A756D64).
const BOX_TYPE_JUMD: u32 = 0x6A75_6D64;
/// CBOR content box type "cbor" (0x63626F72).
const BOX_TYPE_CBOR: u32 = 0x6362_6F72;

/// c2pa.signature UUID (16 bytes).
/// hex: "6332637300110010800000AA00389B71"
const CAI_SIGNATURE_UUID: [u8; 16] = [
    0x63, 0x32, 0x63, 0x73, 0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
    0x71,
];

/// Maximum COSE signature data size (16 MiB).
/// Rejects oversized data to prevent OOM attacks.
const MAX_SIGNATURE_SIZE: u64 = 16 * 1024 * 1024;

/// JUMBF box header.
struct BoxHeader {
    box_type: u32,
    size: u64,
}

/// Description box parsed content.
struct DescInfo {
    uuid: [u8; 16],
    label: String,
}

/// Reads a JUMBF box header.
/// Returns box_type=0, size=0 on EOF.
fn read_header(reader: &mut Cursor<&[u8]>) -> Result<BoxHeader, ProcessorError> {
    let mut buf = [0u8; 8];
    if reader.read(&mut buf).map_err(|e| {
        ProcessorError::C2paVerificationFailed(format!("JUMBF header read error: {e}"))
    })? < 8
    {
        return Ok(BoxHeader {
            box_type: 0,
            size: 0,
        });
    }

    let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let box_type = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);

    if size == 1 {
        // Extended size (u64)
        let mut ext_buf = [0u8; 8];
        reader.read_exact(&mut ext_buf).map_err(|e| {
            ProcessorError::C2paVerificationFailed(format!(
                "JUMBF extended size read error: {e}"
            ))
        })?;
        let large_size = u64::from_be_bytes(ext_buf);
        Ok(BoxHeader {
            box_type,
            size: large_size,
        })
    } else {
        Ok(BoxHeader {
            box_type,
            size: size as u64,
        })
    }
}

/// Reads a description box (UUID + label).
fn read_desc_info(
    reader: &mut Cursor<&[u8]>,
    content_size: u64,
) -> Result<DescInfo, ProcessorError> {
    if content_size < 17 {
        return Err(ProcessorError::C2paVerificationFailed(
            "JUMBF description box too short".to_string(),
        ));
    }

    let mut uuid = [0u8; 16];
    reader.read_exact(&mut uuid).map_err(|e| {
        ProcessorError::C2paVerificationFailed(format!("UUID read error: {e}"))
    })?;

    let mut toggles = [0u8; 1];
    reader.read_exact(&mut toggles).map_err(|e| {
        ProcessorError::C2paVerificationFailed(format!("Toggles read error: {e}"))
    })?;

    let mut label = String::new();
    if toggles[0] & 0x02 != 0 {
        // Null-terminated label string.
        // C2PA labels are ASCII-only, so byte-by-byte reading suffices.
        let max_label_len = (content_size - 17) as usize;
        let mut byte = [0u8; 1];
        loop {
            if label.len() >= max_label_len {
                return Err(ProcessorError::C2paVerificationFailed(
                    "JUMBF label not null-terminated".to_string(),
                ));
            }
            reader.read_exact(&mut byte).map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!("Label read error: {e}"))
            })?;
            if byte[0] == 0 {
                break;
            }
            label.push(byte[0] as char);
        }
    }

    // Skip remaining bytes (padding, salt hash, etc.)
    let read_so_far =
        16 + 1 + if label.is_empty() { 0 } else { label.len() + 1 } as u64;
    if read_so_far < content_size {
        let skip = content_size - read_so_far;
        reader.seek(SeekFrom::Current(skip as i64)).map_err(|e| {
            ProcessorError::C2paVerificationFailed(format!("Skip error: {e}"))
        })?;
    }

    Ok(DescInfo { uuid, label })
}

/// Extracts the COSE signature bytes from JUMBF data for a given manifest label.
///
/// Spec §1.3 — signature_hash = SHA-256(Active Manifest's COSE signature)
///
/// # Arguments
/// * `jumbf_data` — Raw JUMBF bytes from `c2pa::jumbf_io::load_jumbf_from_memory`
/// * `manifest_label` — Target manifest label (from `Reader::active_label()`)
pub fn extract_signature_from_jumbf(
    jumbf_data: &[u8],
    manifest_label: &str,
) -> Result<Vec<u8>, ProcessorError> {
    let mut reader = Cursor::new(jumbf_data);

    // Read top-level superbox (c2pa store)
    let top_header = read_header(&mut reader)?;
    if top_header.box_type != BOX_TYPE_JUMB {
        return Err(ProcessorError::C2paVerificationFailed(
            "Top-level is not a JUMBF superbox".to_string(),
        ));
    }

    // Read description box
    let desc_header = read_header(&mut reader)?;
    if desc_header.box_type != BOX_TYPE_JUMD {
        return Err(ProcessorError::C2paVerificationFailed(
            "Description box not found".to_string(),
        ));
    }
    let _top_desc = read_desc_info(&mut reader, desc_header.size - HEADER_SIZE)?;

    // Scan child superboxes to find the target manifest label
    let top_end = top_header.size;
    while reader.position() < top_end {
        let child_start = reader.position();
        let child_header = read_header(&mut reader)?;
        if child_header.box_type == 0 || child_header.size == 0 {
            break;
        }

        if child_header.box_type == BOX_TYPE_JUMB {
            // Manifest superbox: read description box for label
            let desc_header = read_header(&mut reader)?;
            if desc_header.box_type == BOX_TYPE_JUMD {
                let desc = read_desc_info(&mut reader, desc_header.size - HEADER_SIZE)?;

                if desc.label == manifest_label {
                    // Found the target manifest — find c2pa.signature box inside
                    return find_signature_in_manifest(
                        &mut reader,
                        child_start + child_header.size,
                    );
                }
            }
        }

        // Skip remaining bytes of this box
        reader
            .seek(SeekFrom::Start(child_start + child_header.size))
            .map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!("Seek error: {e}"))
            })?;
    }

    Err(ProcessorError::C2paVerificationFailed(format!(
        "Manifest '{manifest_label}' not found in JUMBF"
    )))
}

/// Finds the c2pa.signature box within a manifest superbox and extracts its CBOR data.
fn find_signature_in_manifest(
    reader: &mut Cursor<&[u8]>,
    manifest_end: u64,
) -> Result<Vec<u8>, ProcessorError> {
    while reader.position() < manifest_end {
        let box_start = reader.position();
        let header = read_header(reader)?;
        if header.box_type == 0 || header.size == 0 {
            break;
        }

        if header.box_type == BOX_TYPE_JUMB {
            // Read description box to check UUID
            let desc_header = read_header(reader)?;
            if desc_header.box_type == BOX_TYPE_JUMD {
                let desc = read_desc_info(reader, desc_header.size - HEADER_SIZE)?;

                if desc.uuid == CAI_SIGNATURE_UUID {
                    // Found c2pa.signature superbox — extract CBOR box
                    return find_cbor_in_box(reader, box_start + header.size);
                }
            }
        }

        // Skip remaining bytes of this box
        reader
            .seek(SeekFrom::Start(box_start + header.size))
            .map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!("Seek error: {e}"))
            })?;
    }

    Err(ProcessorError::C2paVerificationFailed(
        "c2pa.signature box not found".to_string(),
    ))
}

/// Extracts the first CBOR box data from within a superbox.
fn find_cbor_in_box(
    reader: &mut Cursor<&[u8]>,
    box_end: u64,
) -> Result<Vec<u8>, ProcessorError> {
    while reader.position() < box_end {
        let box_start = reader.position();
        let header = read_header(reader)?;
        if header.box_type == 0 || header.size == 0 {
            break;
        }

        if header.box_type == BOX_TYPE_CBOR {
            let data_len = header.size - HEADER_SIZE;
            // Prevent OOM from oversized CBOR data
            if data_len > MAX_SIGNATURE_SIZE {
                return Err(ProcessorError::C2paVerificationFailed(format!(
                    "CBOR box size exceeds limit: {data_len} > {MAX_SIGNATURE_SIZE}"
                )));
            }
            let mut data = vec![0u8; data_len as usize];
            reader.read_exact(&mut data).map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!("CBOR read error: {e}"))
            })?;
            return Ok(data);
        }

        // Skip this box
        reader
            .seek(SeekFrom::Start(box_start + header.size))
            .map_err(|e| {
                ProcessorError::C2paVerificationFailed(format!("Seek error: {e}"))
            })?;
    }

    Err(ProcessorError::C2paVerificationFailed(
        "CBOR box not found in c2pa.signature".to_string(),
    ))
}
