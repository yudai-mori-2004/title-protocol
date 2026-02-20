//! # c2pa-license-v1 Extension WASM モジュール
//!
//! 仕様書 §4.2: C2PAのCreative Workアサーションからライセンス情報を抽出する。
//!
//! ## 処理内容
//! コンテンツのバイナリデータを走査し、Creative Commons等のライセンス情報を検出する。
//! 検出対象:
//! - `schema.org` の CreativeWork アサーション
//! - Creative Commons ライセンスURL
//! - `c2pa.rights` アサーション
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
// ホスト関数宣言（TEEホストが提供）
// ---------------------------------------------------------------------------

extern "C" {
    /// コンテンツの指定範囲をチャンク単位で読み取る。
    fn read_content_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32;

    /// コンテンツの指定範囲に対するハッシュを計算する。
    fn hash_content(algorithm: u32, offset: u32, length: u32, out_ptr: u32) -> u32;

    /// コンテンツの全長を返す。
    fn get_content_length() -> u32;

    /// Extension補助入力を取得する。
    fn get_extension_input(buf_ptr: u32, buf_len: u32) -> u32;
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
// 結果バッファ書き込みヘルパー
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

/// コンテンツ内でバイトパターンを検索する。
fn find_pattern(pattern: &[u8]) -> bool {
    let _ = (hash_content, get_extension_input);

    let content_len = unsafe { get_content_length() } as usize;
    if content_len == 0 || pattern.is_empty() {
        return false;
    }

    const CHUNK_SIZE: usize = 65536;
    let buf = alloc(CHUNK_SIZE as u32);
    if buf == 0 {
        return false;
    }

    let mut offset: usize = 0;
    while offset < content_len {
        let to_read = core::cmp::min(CHUNK_SIZE, content_len - offset);
        let read = unsafe { read_content_chunk(offset as u32, to_read as u32, buf) } as usize;
        if read == 0 {
            break;
        }

        let chunk = unsafe { core::slice::from_raw_parts(buf as *const u8, read) };

        if read >= pattern.len() {
            for i in 0..=(read - pattern.len()) {
                if &chunk[i..i + pattern.len()] == pattern {
                    return true;
                }
            }
        }

        if read >= pattern.len() {
            offset += read - (pattern.len() - 1);
        } else {
            offset += read;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// エクスポート関数
// ---------------------------------------------------------------------------

/// ライセンス情報を抽出する。
/// 仕様書 §4.2
///
/// C2PA Creative Work アサーションおよびライセンス関連マーカーを走査し、
/// 検出されたライセンス種別を返す。
///
/// 返却JSON:
/// - `{"license":"CC-BY-4.0","detected":true}` — Creative Commons検出
/// - `{"license":"rights_reserved","detected":true}` — 権利表示検出
/// - `{"license":"unknown","detected":false}` — ライセンス情報未検出
#[no_mangle]
pub extern "C" fn process() -> u32 {
    // Creative Commonsライセンス各種を検索
    let cc_patterns: &[(&[u8], &str)] = &[
        (b"creativecommons.org/licenses/by/4.0", "CC-BY-4.0"),
        (b"creativecommons.org/licenses/by-sa/4.0", "CC-BY-SA-4.0"),
        (b"creativecommons.org/licenses/by-nc/4.0", "CC-BY-NC-4.0"),
        (
            b"creativecommons.org/licenses/by-nc-sa/4.0",
            "CC-BY-NC-SA-4.0",
        ),
        (
            b"creativecommons.org/licenses/by-nd/4.0",
            "CC-BY-ND-4.0",
        ),
        (
            b"creativecommons.org/licenses/by-nc-nd/4.0",
            "CC-BY-NC-ND-4.0",
        ),
        (
            b"creativecommons.org/publicdomain/zero/1.0",
            "CC0-1.0",
        ),
    ];

    for (pattern, license_id) in cc_patterns {
        if find_pattern(pattern) {
            let mut json = String::with_capacity(64);
            json.push_str("{\"license\":\"");
            json.push_str(license_id);
            json.push_str("\",\"detected\":true}");
            return write_result(&json);
        }
    }

    // c2pa.rights アサーションを検索
    if find_pattern(b"c2pa.rights") {
        return write_result("{\"license\":\"rights_reserved\",\"detected\":true}");
    }

    // schema.org CreativeWork を検索
    if find_pattern(b"schema.org") && find_pattern(b"CreativeWork") {
        return write_result("{\"license\":\"creative_work\",\"detected\":true}");
    }

    // ライセンス情報未検出
    write_result("{\"license\":\"unknown\",\"detected\":false}")
}
