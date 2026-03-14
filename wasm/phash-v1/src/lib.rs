// SPDX-License-Identifier: Apache-2.0

//! # pHash Extension WASM モジュール
//!
//! 仕様書 §3.2: 知覚ハッシュ（perceptual hash）を算出するExtension。
//! ホスト側デコード関数を使用し、あらゆる画像フォーマットに対応する。
//!
//! ## アルゴリズム: pHash (DCT)
//! 1. ホスト側で画像をグレースケールにデコード（`decode_content`）
//! 2. 32×32にバイリニア補間リサイズ
//! 3. 分離型2D DCT（行方向→列方向、O(N³)）
//! 4. 左上8×8低周波ブロックを抽出
//! 5. DC成分を除く63値の平均と比較 → 64bit ハッシュ
//!
//! pHashは画像変換（リサイズ、圧縮、色調補正）に対してロバストなハッシュを返す。
//! ハミング距離が小さいほど類似度が高い。
//!
//! ## 対応フォーマット
//! ホスト側の`image`crateが対応する全フォーマット（JPEG, PNG, WebP, GIF, BMP, TIFF等）
//!
//! ## ターゲット
//! `wasm32-unknown-unknown`

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
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
    fn read_content_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32;

    /// コンテンツの全長を返す。
    fn get_content_length() -> u32;

    /// Extension補助入力を取得する。
    fn get_extension_input(buf_ptr: u32, buf_len: u32) -> u32;

    /// コンテンツをデコードする。
    /// target_format: 0=GRAYSCALE, 1=RGB, 2=RGBA
    /// metadata_ptr: [width:u32 LE, height:u32 LE, channels:u32 LE] を書き込む
    /// 戻り値: 0=成功, -1=非対応, -2=メモリ超過, -3=デコードエラー
    fn decode_content(target_format: u32, params_ptr: u32, params_len: u32, metadata_ptr: u32)
        -> i32;

    /// デコード済みデータの指定範囲を読み取る。
    fn read_decoded_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32;

    /// デコード済みデータの全長を返す。
    fn get_decoded_length() -> u32;
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
// 結果バッファ書き込みヘルパー
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

// ---------------------------------------------------------------------------
// デコード済みピクセル読み取り
// ---------------------------------------------------------------------------

/// ホスト関数経由でデコード済みピクセルデータ全体を読み取る。
fn read_all_decoded() -> Vec<u8> {
    let total = unsafe { get_decoded_length() } as usize;
    if total == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; total];
    let chunk_size: usize = 64 * 1024; // 64KB chunks
    let mut offset: usize = 0;
    while offset < total {
        let remaining = total - offset;
        let len = if remaining < chunk_size {
            remaining
        } else {
            chunk_size
        };
        let buf_ptr = buf.as_mut_ptr() as u32 + offset as u32;
        unsafe {
            read_decoded_chunk(offset as u32, len as u32, buf_ptr);
        }
        offset += len;
    }
    buf
}

// ---------------------------------------------------------------------------
// リサイズ（バイリニア補間）
// ---------------------------------------------------------------------------

/// グレースケール画像を指定サイズにリサイズする。
fn resize_bilinear(
    pixels: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; dst_w * dst_h];

    for y in 0..dst_h {
        for x in 0..dst_w {
            let src_x = (x as f32 + 0.5) * (src_w as f32 / dst_w as f32) - 0.5;
            let src_y = (y as f32 + 0.5) * (src_h as f32 / dst_h as f32) - 0.5;

            let x0 = libm::floorf(src_x) as i32;
            let y0 = libm::floorf(src_y) as i32;
            let x1 = x0 + 1;
            let y1 = y0 + 1;

            let fx = src_x - x0 as f32;
            let fy = src_y - y0 as f32;

            let get = |px: i32, py: i32| -> f32 {
                let px = if px < 0 {
                    0
                } else if px >= src_w as i32 {
                    src_w - 1
                } else {
                    px as usize
                };
                let py = if py < 0 {
                    0
                } else if py >= src_h as i32 {
                    src_h - 1
                } else {
                    py as usize
                };
                pixels[py * src_w + px] as f32
            };

            let v = get(x0, y0) * (1.0 - fx) * (1.0 - fy)
                + get(x1, y0) * fx * (1.0 - fy)
                + get(x0, y1) * (1.0 - fx) * fy
                + get(x1, y1) * fx * fy;

            out[y * dst_w + x] = libm::roundf(v) as u8;
        }
    }

    out
}

// ---------------------------------------------------------------------------
// pHash (DCT) — 64bit
// ---------------------------------------------------------------------------

/// DCTサイズ
const DCT_SIZE: usize = 32;
/// 低周波ブロックサイズ
const LOW_FREQ: usize = 8;

/// pHash: 32×32にリサイズ → 分離型2D DCT → 8×8低周波ブロック → 平均比較 → 64bitハッシュ
/// 仕様書 §7.4
fn compute_phash_dct(pixels: &[u8], width: u32, height: u32) -> u64 {
    // 1. 32×32にバイリニア補間リサイズ
    let resized = resize_bilinear(pixels, width as usize, height as usize, DCT_SIZE, DCT_SIZE);

    // f32に変換
    let mut matrix = [[0.0f32; DCT_SIZE]; DCT_SIZE];
    for y in 0..DCT_SIZE {
        for x in 0..DCT_SIZE {
            matrix[y][x] = resized[y * DCT_SIZE + x] as f32;
        }
    }

    // 2. 分離型2D DCT: 行方向1D DCT → 列方向1D DCT
    let n = DCT_SIZE as f32;
    let scale = libm::sqrtf(2.0 / n);
    let inv_sqrt2 = 1.0 / libm::sqrtf(2.0);

    // 行方向 DCT
    let mut row_dct = [[0.0f32; DCT_SIZE]; DCT_SIZE];
    for y in 0..DCT_SIZE {
        for u in 0..DCT_SIZE {
            let cu = if u == 0 { inv_sqrt2 } else { 1.0 };
            let mut sum = 0.0f32;
            for x in 0..DCT_SIZE {
                sum += matrix[y][x]
                    * libm::cosf(
                        core::f32::consts::PI * (2.0 * x as f32 + 1.0) * u as f32 / (2.0 * n),
                    );
            }
            row_dct[y][u] = sum * cu * scale;
        }
    }

    // 列方向 DCT
    let mut dct = [[0.0f32; DCT_SIZE]; DCT_SIZE];
    for u in 0..DCT_SIZE {
        for v in 0..DCT_SIZE {
            let cv = if v == 0 { inv_sqrt2 } else { 1.0 };
            let mut sum = 0.0f32;
            for y in 0..DCT_SIZE {
                sum += row_dct[y][u]
                    * libm::cosf(
                        core::f32::consts::PI * (2.0 * y as f32 + 1.0) * v as f32 / (2.0 * n),
                    );
            }
            dct[v][u] = sum * cv * scale;
        }
    }

    // 3. 左上8×8低周波ブロックを抽出
    let mut values = [0.0f32; LOW_FREQ * LOW_FREQ];
    for v in 0..LOW_FREQ {
        for u in 0..LOW_FREQ {
            values[v * LOW_FREQ + u] = dct[v][u];
        }
    }

    // 4. DC成分（values[0]）を除く63値の平均を計算
    let sum: f32 = values[1..].iter().copied().sum();
    let mean = sum / (LOW_FREQ * LOW_FREQ - 1) as f32;

    // 5. 各値を平均と比較 → 64bitハッシュ
    let mut hash: u64 = 0;
    for i in 0..64 {
        if values[i] > mean {
            hash |= 1u64 << i;
        }
    }

    hash
}

// ---------------------------------------------------------------------------
// エクスポート関数
// ---------------------------------------------------------------------------

/// 知覚ハッシュ（pHash-DCT）を計算する。
/// 仕様書 §3.2
///
/// ホスト側で画像をグレースケールにデコードし、pHash (DCT) アルゴリズムで
/// 64bitの知覚ハッシュを計算する。
///
/// 結果JSON: {"phash":"<16桁hex>","algorithm":"phash-dct","bits":64}
#[no_mangle]
pub extern "C" fn process() -> u32 {
    // suppress unused warnings
    let _ = (get_extension_input, read_content_chunk, get_content_length);

    // 1. ホスト側でグレースケールにデコード
    let mut metadata = [0u8; 12];
    let rc = unsafe { decode_content(0, 0, 0, metadata.as_mut_ptr() as u32) };

    match rc {
        0 => {} // 成功
        -1 => {
            return write_result(
                "{\"error\":\"unsupported image format (supported: JPEG, PNG, WebP, GIF, BMP, TIFF)\"}",
            );
        }
        -2 => {
            return write_result("{\"error\":\"memory budget exceeded\"}");
        }
        _ => {
            return write_result("{\"error\":\"decode error\"}");
        }
    }

    // 2. メタデータからサイズ取得
    let width = u32::from_le_bytes([metadata[0], metadata[1], metadata[2], metadata[3]]);
    let height = u32::from_le_bytes([metadata[4], metadata[5], metadata[6], metadata[7]]);

    if width == 0 || height == 0 {
        return write_result("{\"error\":\"invalid image dimensions\"}");
    }

    // 3. デコード済みピクセルを全読み取り
    let pixels = read_all_decoded();
    if pixels.is_empty() {
        return write_result("{\"error\":\"empty decoded data\"}");
    }

    // 4. pHash (DCT) 計算
    let hash = compute_phash_dct(&pixels, width, height);

    // 5. 結果JSON構築
    let mut hex = String::with_capacity(16);
    let _ = write!(&mut hex, "{:016x}", hash);

    let mut json = String::with_capacity(80);
    json.push_str("{\"phash\":\"");
    json.push_str(&hex);
    json.push_str("\",\"algorithm\":\"phash-dct\",\"bits\":64}");

    write_result(&json)
}
