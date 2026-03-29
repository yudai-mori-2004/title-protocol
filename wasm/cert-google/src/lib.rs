// SPDX-License-Identifier: Apache-2.0

//! # cert-google Extension WASM モジュール
//!
//! C2PAアクティブマニフェストの署名証明書チェーンを
//! Google C2PA Root CA G3 に対して暗号的に検証する。
//!
//! ## Root CA
//! - Subject: CN=Google C2PA Root CA G3, O=Google LLC, C=US
//! - アルゴリズム: ECDSA P-384 + SHA-384
//! - ソース: C2PA公式Trust List + http://pki.goog/c2pa/root-g3.crt
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

/// Google C2PA Root CA G3 の SubjectPublicKeyInfo (DER, hex)
/// ソース: http://pki.goog/c2pa/root-g3.crt
/// C2PA公式Trust List および ITL anchors.pem と一致確認済み
const ROOT_SPKI_HEX: &str = "\
    3076301006072a8648ce3d020106052b8104002203620004\
    86ff5ffe3b8a70fa5edc59bb78021232e4b24beb41c6\
    7d1a6070bcdc9faa02c15644418df69e8f37f381a28b8f\
    ce9385471beb956a16980237a75957c8f8381377a0ed23\
    42860a29508a62846bbaaa584ff2b2d77f7a7c6e123915\
    343631a176";

const ROOT_CA_NAME: &str = "Google C2PA Root CA G3";

// ---------------------------------------------------------------------------
// ホスト関数宣言（TEEホストが提供）
// ---------------------------------------------------------------------------

extern "C" {
    /// コンテンツの特徴量を計算する（JSON spec指定）。
    /// 仕様書 §7.1
    fn get_content_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32;
}

// ---------------------------------------------------------------------------
// メモリアロケータ
// ---------------------------------------------------------------------------

/// WASMモジュール用のメモリアロケーション関数。
#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let layout = core::alloc::Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc::alloc::alloc(layout) as u32 }
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

/// JSON文字列を length-prefixed 結果バッファとして書き込み、ポインタを返す。
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


/// JSON文字列内の特殊文字をエスケープする。
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

/// C2PA証明書チェーンを Google C2PA Root CA G3 に対して検証する。
#[no_mangle]
pub extern "C" fn process() -> u32 {
    // ホスト関数に渡すJSON spec
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

    // 出力バッファ（ホスト関数からのJSON結果を受け取る）
    const OUTPUT_BUF_SIZE: u32 = 8192;
    let output_ptr = alloc(OUTPUT_BUF_SIZE);
    if output_ptr == 0 {
        return write_result(r#"{"verified":false,"error":"alloc failed"}"#);
    }

    // ホスト関数呼び出し
    let result_len = unsafe {
        get_content_feature(spec_ptr, spec_bytes.len() as u32, output_ptr)
    };

    if result_len < 0 {
        // エラーコード
        return write_result(r#"{"verified":false,"error":"cert chain verification failed"}"#);
    }

    // ホストからのJSON結果を読み取り
    let result_bytes = unsafe {
        core::slice::from_raw_parts(output_ptr as *const u8, result_len as usize)
    };

    // ホスト結果からverifiedとchainを抽出して最終結果JSONを構築
    // ホスト結果: {"verified":true/false,"chain":[{"subject":"..."},...]}}
    // 最終結果に root_ca と root_spki を追加
    let host_json = match core::str::from_utf8(result_bytes) {
        Ok(s) => s,
        Err(_) => return write_result(r#"{"verified":false,"error":"invalid utf8"}"#),
    };


    // ホストJSONをパースせずに結果を構築（no_stdでJSONパーサーがないため）
    // ホストの結果はそのまま展開し、root_caとroot_spkiを追加
    // ホスト結果の末尾の '}' を除去し、追加フィールドを付加
    let mut final_json = String::with_capacity(host_json.len() + 200);
    if let Some(stripped) = host_json.strip_suffix('}') {
        final_json.push_str(stripped);
        final_json.push_str(",\"root_ca\":\"");
        json_escape(ROOT_CA_NAME, &mut final_json);
        final_json.push_str("\",\"root_spki\":\"");
        final_json.push_str(ROOT_SPKI_HEX);
        final_json.push_str("\"}");
    } else {
        // ホスト結果が予期しない形式の場合はそのまま返す
        final_json.push_str(host_json);
    }

    write_result(&final_json)
}
