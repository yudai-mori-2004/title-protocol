//! # c2pa-license-v1 Extension WASM モジュール
//!
//! 仕様書 §4.2: C2PAのCreative Workアサーションからライセンス情報を抽出する。
//!
//! ## ターゲット
//! `wasm32-unknown-unknown`

#![no_std]

extern crate alloc;

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
// エクスポート関数
// ---------------------------------------------------------------------------

/// ライセンス情報を抽出する。
/// 仕様書 §4.2
///
/// TODO: C2PA Creative Workアサーションの抽出
#[no_mangle]
pub extern "C" fn extract_license() -> u32 {
    let _ = (read_content_chunk, hash_content);
    0
}
