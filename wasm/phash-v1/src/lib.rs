//! # pHash Extension WASM モジュール
//!
//! 仕様書 §3.2: 知覚ハッシュを算出する内部完結型Extension。
//! コンテンツの生データのみで動作し、補助入力は不要。
//!
//! ## 初期実装
//! SHA-256ベースの簡易ハッシュで代替（仕様書許容）。
//! 将来: DCTベースの知覚ハッシュに置換。
//!
//! ## ターゲット
//! `wasm32-unknown-unknown`
//!
//! ## ホスト関数
//! - `read_content_chunk(offset, length, buf_ptr)`: コンテンツのチャンク読み取り
//! - `hash_content(algorithm, offset, length, out_ptr)`: コンテンツのハッシュ計算
//! - `get_content_length()`: コンテンツの全長取得

#![no_std]

extern crate alloc;

use alloc::string::String;
use core::fmt::Write;

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ---------------------------------------------------------------------------
// ホスト関数宣言（TEEホストが提供）
// 仕様書 §7.1
// ---------------------------------------------------------------------------

extern "C" {
    /// コンテンツの指定範囲をチャンク単位で読み取る。
    /// 仕様書 §7.1
    fn read_content_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32;

    /// コンテンツの指定範囲に対するハッシュを計算する。
    /// algorithm: 0=sha256, 1=sha384, 2=sha512
    /// 仕様書 §7.1
    fn hash_content(algorithm: u32, offset: u32, length: u32, out_ptr: u32) -> u32;

    /// コンテンツの全長を返す。
    fn get_content_length() -> u32;

    /// Extension補助入力を取得する。
    /// 仕様書 §7.1
    fn get_extension_input(buf_ptr: u32, buf_len: u32) -> u32;
}

// ---------------------------------------------------------------------------
// メモリアロケータ
// ---------------------------------------------------------------------------

/// WASMモジュール用のメモリアロケーション関数。
/// ホストがWASMメモリにデータを書き込むために使用する。
#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let layout = core::alloc::Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc::alloc::alloc(layout) as u32 }
}

// ---------------------------------------------------------------------------
// 結果バッファ書き込みヘルパー
// ---------------------------------------------------------------------------

/// JSON文字列を length-prefixed 結果バッファとして書き込み、ポインタを返す。
/// バッファ形式: [4B LE: json_len][json_bytes...]
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

// ---------------------------------------------------------------------------
// エクスポート関数
// ---------------------------------------------------------------------------

/// 知覚ハッシュを計算する。
/// 仕様書 §3.2
///
/// 初期実装: SHA-256(コンテンツ全体)をhex文字列として返す。
/// 将来: 画像をグレースケール8x8に縮小 → DCT → 中央値比較 → 64bitハッシュ
#[no_mangle]
pub extern "C" fn process() -> u32 {
    let _ = (read_content_chunk, get_extension_input);

    let content_len = unsafe { get_content_length() };
    if content_len == 0 {
        return 0;
    }

    // SHA-256ハッシュ用バッファ（32バイト）をアロケート
    let hash_buf = alloc(32);
    if hash_buf == 0 {
        return 0;
    }

    // SHA-256(コンテンツ全体)をホスト関数で計算
    // 仕様書 §7.1: hash_content でネイティブ速度のハッシュ計算
    let hash_size = unsafe { hash_content(0, 0, content_len, hash_buf) };
    if hash_size != 32 {
        return 0;
    }

    // ハッシュをhex文字列に変換
    let hash_slice = unsafe { core::slice::from_raw_parts(hash_buf as *const u8, 32) };
    let mut hex = String::with_capacity(64);
    for &b in hash_slice {
        let _ = write!(&mut hex, "{:02x}", b);
    }

    // JSON結果を構築: {"phash":"<hex>"}
    let mut json = String::with_capacity(80);
    json.push_str("{\"phash\":\"");
    json.push_str(&hex);
    json.push_str("\"}");

    write_result(&json)
}
