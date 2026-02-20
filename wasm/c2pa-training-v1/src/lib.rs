//! # c2pa-training-v1 Extension WASM モジュール
//!
//! 仕様書 §4.2: C2PAの`c2pa.training-mining`アサーションからAI学習許諾フラグを抽出する。
//!
//! ## 処理内容
//! コンテンツのバイナリデータを走査し、`c2pa.training-mining`アサーションの
//! 存在を検出する。アサーション内の`use`フィールドの値に基づき、
//! AI学習許可（allowed）/ 禁止（notAllowed）を判定する。
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

/// コンテンツ内でバイトパターンを検索する。見つかった場合、周辺のコンテキストバイトも返す。
fn find_pattern_with_context(pattern: &[u8], context_after: usize) -> Option<u32> {
    let _ = (hash_content, get_extension_input);

    let content_len = unsafe { get_content_length() } as usize;
    if content_len == 0 || pattern.is_empty() {
        return None;
    }

    const CHUNK_SIZE: usize = 65536;
    let buf = alloc(CHUNK_SIZE as u32);
    if buf == 0 {
        return None;
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
                    // パターン後のコンテキストバイトを別バッファにコピー
                    let ctx_start = offset + i + pattern.len();
                    let ctx_len = core::cmp::min(context_after, content_len - ctx_start);
                    if ctx_len > 0 {
                        let ctx_buf = alloc(ctx_len as u32);
                        if ctx_buf != 0 {
                            unsafe {
                                read_content_chunk(ctx_start as u32, ctx_len as u32, ctx_buf);
                            }
                            return Some(ctx_buf);
                        }
                    }
                    return Some(0);
                }
            }
        }

        if read >= pattern.len() {
            offset += read - (pattern.len() - 1);
        } else {
            offset += read;
        }
    }

    None
}

// ---------------------------------------------------------------------------
// エクスポート関数
// ---------------------------------------------------------------------------

/// AI学習許諾フラグを抽出する。
/// 仕様書 §4.2
///
/// C2PA `c2pa.training-mining` アサーションを走査し、
/// AI学習許可/禁止のフラグを返す。
///
/// 返却JSON:
/// - `{"training_allowed":true}` — 学習許可
/// - `{"training_allowed":false}` — 学習禁止
/// - `{"training_allowed":null,"reason":"not_found"}` — アサーション未検出
#[no_mangle]
pub extern "C" fn process() -> u32 {
    // "c2pa.training-mining" アサーションマーカーを検索
    // 見つかった場合、後続のコンテキストで "notAllowed" を検索
    let marker = b"c2pa.training-mining";

    match find_pattern_with_context(marker, 256) {
        Some(ctx_buf) => {
            if ctx_buf != 0 {
                // コンテキスト内で "notAllowed" を探す
                let ctx = unsafe { core::slice::from_raw_parts(ctx_buf as *const u8, 256) };
                let not_allowed = b"notAllowed";
                let mut found_not_allowed = false;

                if ctx.len() >= not_allowed.len() {
                    for i in 0..=(ctx.len() - not_allowed.len()) {
                        if &ctx[i..i + not_allowed.len()] == &not_allowed[..] {
                            found_not_allowed = true;
                            break;
                        }
                    }
                }

                if found_not_allowed {
                    write_result("{\"training_allowed\":false}")
                } else {
                    // アサーションは存在するが notAllowed でない → 許可
                    write_result("{\"training_allowed\":true}")
                }
            } else {
                // マーカーは見つかったがコンテキスト取得に失敗 → 存在するので判定
                write_result("{\"training_allowed\":true}")
            }
        }
        None => {
            // アサーション未検出
            write_result("{\"training_allowed\":null,\"reason\":\"not_found\"}")
        }
    }
}
