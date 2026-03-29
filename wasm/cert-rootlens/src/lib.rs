// SPDX-License-Identifier: Apache-2.0

//! # cert-rootlens Extension WASM モジュール
//!
//! C2PAアクティブマニフェストの署名証明書チェーンを
//! RootLens Root CA に対して暗号的に検証する。
//!
//! ## Root CA
//! - Subject: CN=RootLens Root CA, O=RootLens, C=JP
//! - アルゴリズム: ECDSA P-256 + SHA-256
//! - 3層PKI: Root → Platform ICA (iOS/Android) → Device Cert
//!
//! ## 注意
//! 本バイナリにはDev Root CAのSPKIがハードコードされている。
//! 本番Root CA生成後は add_wasm_version で新バージョンを登録すること。
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

/// RootLens Dev Root CA の SubjectPublicKeyInfo (DER, hex)
/// ソース: root-lens/certs/dev/root-ca.pem
/// 本番環境では add_wasm_version で Prod Root CA 版に切り替える
const ROOT_SPKI_HEX: &str = "\
    3059301306072a8648ce3d020106082a8648ce3d03010703420004\
    da1dc99b9b680e7c97242fe229746a56d0f43bd999c16b299593\
    24604eb6520d950e18bde6bf12c75394bde14b33880bb60fff99\
    071a65db98e9ff9fe48f4a08";

const ROOT_CA_NAME: &str = "RootLens Root CA";

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

/// C2PA証明書チェーンを RootLens Root CA に対して検証する。
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


    let mut final_json = String::with_capacity(host_json.len() + 200);
    if let Some(stripped) = host_json.strip_suffix('}') {
        final_json.push_str(stripped);
        final_json.push_str(",\"root_ca\":\"");
        json_escape(ROOT_CA_NAME, &mut final_json);
        final_json.push_str("\",\"root_spki\":\"");
        final_json.push_str(ROOT_SPKI_HEX);
        final_json.push_str("\"}");
    } else {
        final_json.push_str(host_json);
    }

    write_result(&final_json)
}
