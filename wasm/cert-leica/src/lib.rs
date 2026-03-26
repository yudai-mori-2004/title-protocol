// SPDX-License-Identifier: Apache-2.0

//! # cert-leica Extension WASM モジュール
//!
//! C2PAアクティブマニフェストの署名証明書チェーンを
//! Leica C2PA Root CA に対して暗号的に検証する。
//!
//! ## Root CA
//! - Subject: CN=Leica C2PA Root CA, O=Leica Camera AG, C=DE
//! - アルゴリズム: ECDSA P-256 + SHA-256
//! - PKI運営: D-Trust GmbH (ドイツ連邦印刷局子会社)
//! - ソース: C2PA Interim Trust List (ITL) anchors.pem
//!
//! ## ターゲット
//! `wasm32-unknown-unknown`

#![no_std]

extern crate alloc;

use alloc::string::String;

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ---------------------------------------------------------------------------
// 定数
// ---------------------------------------------------------------------------

/// Leica C2PA Root CA の SubjectPublicKeyInfo (DER, hex)
/// ソース: C2PA Interim Trust List (ITL) anchors.pem
const ROOT_SPKI_HEX: &str = "\
    3059301306072a8648ce3d020106082a8648ce3d03010703420004\
    d858764b362367ba0dce56e372d036dd4c51030ee55fcd97aa8a\
    cdb25f6020a99f5f33647b52a0b4d6fe69331fcdc5131257f43f\
    0c3eb0474770b645dac68864";

const ROOT_CA_NAME: &str = "Leica C2PA Root CA";

// ---------------------------------------------------------------------------
// ホスト関数宣言（TEEホストが提供）
// ---------------------------------------------------------------------------

extern "C" {
    fn get_content_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32;
}

// ---------------------------------------------------------------------------
// メモリアロケータ
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let layout = core::alloc::Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc::alloc::alloc(layout) as u32 }
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

fn write_result(json: &str) -> u32 {
    let json_bytes = json.as_bytes();
    let total = 4 + json_bytes.len();
    let ptr = alloc(total as u32);
    if ptr == 0 {
        return 0;
    }
    let len_bytes = (json_bytes.len() as u32).to_le_bytes();
    unsafe {
        let p = ptr as *mut u8;
        core::ptr::copy_nonoverlapping(len_bytes.as_ptr(), p, 4);
        core::ptr::copy_nonoverlapping(json_bytes.as_ptr(), p.add(4), json_bytes.len());
    }
    ptr
}

fn sha256_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(32);
    for &b in &data[..16.min(data.len())] {
        let hi = b >> 4;
        let lo = b & 0x0f;
        s.push(char::from(if hi < 10 { b'0' + hi } else { b'a' + hi - 10 }));
        s.push(char::from(if lo < 10 { b'0' + lo } else { b'a' + lo - 10 }));
    }
    s
}

fn json_escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// エクスポート関数
// ---------------------------------------------------------------------------

/// C2PA証明書チェーンを Leica C2PA Root CA に対して検証する。
#[no_mangle]
pub extern "C" fn process() -> u32 {
    let spec = alloc::format!(
        r#"{{"op":"c2pa_verify_active_cert_chain","root_spki_hex":"{ROOT_SPKI_HEX}"}}"#
    );
    let spec_bytes = spec.as_bytes();
    let spec_ptr = alloc(spec_bytes.len() as u32);
    if spec_ptr == 0 {
        return write_result(r#"{"verified":false,"error":"alloc failed"}"#);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(spec_bytes.as_ptr(), spec_ptr as *mut u8, spec_bytes.len());
    }

    const OUTPUT_BUF_SIZE: u32 = 8192;
    let output_ptr = alloc(OUTPUT_BUF_SIZE);
    if output_ptr == 0 {
        return write_result(r#"{"verified":false,"error":"alloc failed"}"#);
    }

    let result_len = unsafe {
        get_content_feature(spec_ptr, spec_bytes.len() as u32, output_ptr)
    };

    if result_len < 0 {
        return write_result(r#"{"verified":false,"error":"cert chain verification failed"}"#);
    }

    let result_bytes = unsafe {
        core::slice::from_raw_parts(output_ptr as *const u8, result_len as usize)
    };

    let host_json = match core::str::from_utf8(result_bytes) {
        Ok(s) => s,
        Err(_) => return write_result(r#"{"verified":false,"error":"invalid utf8"}"#),
    };

    let spki_fingerprint = sha256_hex(ROOT_SPKI_HEX.as_bytes());

    let mut final_json = String::with_capacity(host_json.len() + 200);
    if let Some(stripped) = host_json.strip_suffix('}') {
        final_json.push_str(stripped);
        final_json.push_str(",\"root_ca\":\"");
        json_escape(ROOT_CA_NAME, &mut final_json);
        final_json.push_str("\",\"root_spki_hash\":\"");
        final_json.push_str(&spki_fingerprint);
        final_json.push_str("\"}");
    } else {
        final_json.push_str(host_json);
    }

    write_result(&final_json)
}
