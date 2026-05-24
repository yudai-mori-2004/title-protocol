// SPDX-License-Identifier: Apache-2.0

//! # SP1 Guest: AWS Nitro Attestation Document Verifier
//!
//! Spec §6.2 — runs once when a signer key is registered on-chain.
//!
//! Runs inside the SP1 zkVM and proves:
//!  1. The COSE_Sign1 signature on the Attestation Document is valid
//!  2. The certificate chain back to the AWS Nitro PKI root is intact
//!  3. The extracted PCR0 / user_data / public_key fields are not tampered with
//!
//! Public values committed by this guest, in commit order:
//!
//!   instance_id_len   : u32 LE
//!   instance_id       : instance_id_len bytes (UTF-8)
//!   timestamp_ms      : u64 LE
//!   measurement_len   : u32 LE
//!   measurement       : measurement_len bytes (AWS Nitro PCR0 = 48 bytes)
//!   has_user_data     : u8 (0 or 1)
//!   user_data_hash    : 32 bytes (only if has_user_data == 1)
//!   has_public_key    : u8 (0 or 1)
//!   public_key_hash   : 32 bytes (only if has_public_key == 1)
//!
//! NOTE: `instance_id` の長さプレフィックスは必ず `commit(&len_u32)` +
//! `commit_slice(bytes)` の手動書き出しで u32 LE にする。`commit(&String)`
//! を使うと sp1-lib 内部の bincode-fixint シリアライザが u64 LE 長前置
//! (8 バイト) を書き出してしまい、on-chain `parse_public_values` (u32 LE
//! 前提) と 4 バイトずれて register_key が永久に失敗する。
//!
//! `instance_id` is the vendor-neutral name for the device-identifier field;
//! AWS Nitro carries it inside `AttestationDocument::module_id`. Other vendors
//! emit the same envelope from their own equivalent.
//!
//! `measurement_len` is length-prefixed (instead of hard-coded 48) so the
//! on-chain parser is shared across vendors. Per-vendor guests still emit
//! the same overall envelope; only the embedded measurement length differs.

#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};
use title_attestation_aws_nitro::AttestationReport;

/// Hard cap on the COSE_Sign1 document size. Real AWS Nitro documents are
/// well under 8 KiB; a much larger input is either operator error or an
/// attempt to burn cycles inside the zkVM.
const MAX_DOC_BYTES: usize = 16 * 1024;

pub fn main() {
    let doc_bytes: Vec<u8> = sp1_zkvm::io::read_vec();
    assert!(
        doc_bytes.len() <= MAX_DOC_BYTES,
        "attestation document too large: {} > {} bytes",
        doc_bytes.len(),
        MAX_DOC_BYTES,
    );

    let report = AttestationReport::parse(&doc_bytes).expect("COSE_Sign1 parse failed");
    let doc = report.doc();

    // Full cabundle chain. SP1 guests have no wall clock — verify
    // certificate validity against the document's own timestamp.
    report
        .authenticate(doc.timestamp / 1000)
        .expect("Attestation Document verification failed");

    // `doc.module_id` is AWS Nitro's wire name; commit it under the
    // vendor-neutral `instance_id` slot in the public-values envelope.
    // 長さは u32 LE で明示する (commit(&String) を使うと bincode-fixint が
    // u64 LE 長前置を出してしまい on-chain parser とずれる)。
    let id_bytes = doc.module_id.as_bytes();
    sp1_zkvm::io::commit(&(id_bytes.len() as u32));
    sp1_zkvm::io::commit_slice(id_bytes);
    sp1_zkvm::io::commit(&doc.timestamp);

    let measurement = doc.pcrs.get(&0).expect("PCR0 missing");
    let measurement_bytes: &[u8] = measurement.as_ref();
    sp1_zkvm::io::commit(&(measurement_bytes.len() as u32));
    sp1_zkvm::io::commit_slice(measurement_bytes);

    let has_user_data: u8 = doc.user_data.is_some() as u8;
    sp1_zkvm::io::commit(&has_user_data);
    if let Some(ud) = doc.user_data.as_ref() {
        let hash = Sha256::digest(ud.as_ref());
        sp1_zkvm::io::commit_slice(&hash);
    }

    let has_public_key: u8 = doc.public_key.is_some() as u8;
    sp1_zkvm::io::commit(&has_public_key);
    if let Some(pk) = doc.public_key.as_ref() {
        let hash = Sha256::digest(pk.as_ref());
        sp1_zkvm::io::commit_slice(&hash);
    }
}
