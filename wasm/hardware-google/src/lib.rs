//! # hardware-google Extension WASM モジュール
//!
//! 仕様書 §4.2: C2PAのハードウェア署名チェーンを検証し、
//! Google Pixel等のTitan M2チップ搭載端末で撮影されたことを証明する。
//!
//! ## 初期実装
//! コンテンツ内のハードウェア署名アサーション（c2pa.hash.data）の有無を判定する。
//! 将来: Google Titan M2の署名チェーンをフル検証。
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
/// チャンク読み取りで大容量コンテンツにも対応。
fn find_pattern(pattern: &[u8]) -> bool {
    let _ = (hash_content, get_extension_input);

    let content_len = unsafe { get_content_length() } as usize;
    if content_len == 0 || pattern.is_empty() {
        return false;
    }

    // 64KBチャンクで読み取り、パターンを検索
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

        // チャンク内でパターン検索（単純なバイト比較）
        if read >= pattern.len() {
            for i in 0..=(read - pattern.len()) {
                if &chunk[i..i + pattern.len()] == pattern {
                    return true;
                }
            }
        }

        // オーバーラップ対策: パターン長-1だけ手前から次チャンクを読む
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

/// ハードウェア署名を検証する。
/// 仕様書 §4.2
///
/// 初期実装: C2PAマニフェスト内のハードウェア関連アサーションマーカーの有無を検出する。
/// 検出対象: "c2pa.hash.data"（ハードウェアバインディング）、"stds.iptc"（Exif由来メタデータ）
#[no_mangle]
pub extern "C" fn process() -> u32 {
    // ハードウェアバインディングのマーカーを検索
    let has_hash_data = find_pattern(b"c2pa.hash.data");
    // IPTC/Exifメタデータ（カメラ情報を含む可能性）
    let has_iptc = find_pattern(b"stds.iptc");

    let detected = has_hash_data || has_iptc;

    // JSON結果を構築
    let mut json = String::with_capacity(64);
    json.push_str("{\"hardware_detected\":");
    if detected {
        json.push_str("true");
    } else {
        json.push_str("false");
    }
    json.push('}');

    write_result(&json)
}
