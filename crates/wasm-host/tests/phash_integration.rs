// SPDX-License-Identifier: Apache-2.0

//! # phash-v1 統合テスト
//!
//! コンパイル済み phash-v1.wasm を WasmRunner で実行し、
//! pHash (DCT) アルゴリズムの正確性を検証する。
//!
//! ## 前提条件
//! ```bash
//! cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release
//! ```
//!
//! WASM バイナリが存在しない場合、テストはスキップされる。

use std::io::Cursor;

use title_wasm_host::WasmRunner;

/// phash-v1.wasm のパス（CARGO_MANIFEST_DIR からの相対）
const WASM_RELATIVE: &str =
    "../../wasm/phash-v1/target/wasm32-unknown-unknown/release/phash_v1.wasm";

/// phash-v1.wasm をロードする。ビルドされていなければ None。
fn load_phash_wasm() -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/{WASM_RELATIVE}");
    std::fs::read(path).ok()
}

/// 画像バイト列から pHash を実行し、64bit ハッシュを返す。
fn run_phash(wasm: &[u8], image_bytes: &[u8]) -> u64 {
    let runner = WasmRunner::new(100_000_000, 64 * 1024 * 1024);
    let result = runner
        .execute(wasm, image_bytes, None, "process")
        .expect("phash-v1 WASM実行に失敗");

    let hash_str = result.output["phash"]
        .as_str()
        .expect("phash フィールドが見つからない");
    u64::from_str_radix(hash_str, 16).expect("phash の16進パースに失敗")
}

/// ハミング距離を計算する。
fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// グラデーション画像を生成しエンコードする。
fn create_gradient(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        let r = (x * 255 / width.max(1)) as u8;
        let g = (y * 255 / height.max(1)) as u8;
        image::Rgb([r, g, 128])
    });
    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, format)
        .unwrap();
    buf.into_inner()
}

/// チェッカーボード画像を生成しエンコードする。
fn create_checkerboard(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgb([255, 255, 255])
        } else {
            image::Rgb([0, 0, 0])
        }
    });
    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, format)
        .unwrap();
    buf.into_inner()
}

/// 同一画像の JPEG / PNG エンコードが同じ pHash を返すこと。
/// JPEG は非可逆圧縮のため完全一致はしないが、ハミング距離 ≤ 5 であるべき。
#[test]
fn test_phash_same_image_jpeg_png() {
    let wasm = match load_phash_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: phash-v1.wasm が見つかりません（先にビルドしてください）");
            return;
        }
    };

    let jpeg = create_gradient(128, 128, image::ImageFormat::Jpeg);
    let png = create_gradient(128, 128, image::ImageFormat::Png);

    let hash_jpeg = run_phash(&wasm, &jpeg);
    let hash_png = run_phash(&wasm, &png);
    let dist = hamming_distance(hash_jpeg, hash_png);

    eprintln!("JPEG hash: {:016x}, PNG hash: {:016x}, distance: {dist}", hash_jpeg, hash_png);
    assert!(
        dist <= 5,
        "同一画像の JPEG/PNG で hamming distance {dist} > 5"
    );
}

/// リサイズ後の画像が近い pHash を返すこと。
/// pHash はリサイズに対してロバストであるべき。
#[test]
fn test_phash_resize_robustness() {
    let wasm = match load_phash_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: phash-v1.wasm が見つかりません");
            return;
        }
    };

    let large = create_gradient(256, 256, image::ImageFormat::Png);
    let small = create_gradient(64, 64, image::ImageFormat::Png);

    let hash_large = run_phash(&wasm, &large);
    let hash_small = run_phash(&wasm, &small);
    let dist = hamming_distance(hash_large, hash_small);

    eprintln!(
        "256x256 hash: {:016x}, 64x64 hash: {:016x}, distance: {dist}",
        hash_large, hash_small
    );
    assert!(
        dist <= 5,
        "リサイズ画像の hamming distance {dist} > 5"
    );
}

/// 異なる画像が異なる pHash を返すこと。
/// 視覚的に異なる画像はハミング距離 ≥ 20 であるべき。
#[test]
fn test_phash_different_images() {
    let wasm = match load_phash_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: phash-v1.wasm が見つかりません");
            return;
        }
    };

    let gradient = create_gradient(128, 128, image::ImageFormat::Png);
    let checker = create_checkerboard(128, 128, image::ImageFormat::Png);

    let hash_gradient = run_phash(&wasm, &gradient);
    let hash_checker = run_phash(&wasm, &checker);
    let dist = hamming_distance(hash_gradient, hash_checker);

    eprintln!(
        "gradient hash: {:016x}, checker hash: {:016x}, distance: {dist}",
        hash_gradient, hash_checker
    );
    assert!(
        dist >= 20,
        "異なる画像の hamming distance {dist} < 20"
    );
}

/// pHash 計算が決定的であること。
/// 同一入力に対して常に同じハッシュを返す（DCT 数値精度の検証）。
#[test]
fn test_phash_deterministic() {
    let wasm = match load_phash_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: phash-v1.wasm が見つかりません");
            return;
        }
    };

    let png = create_gradient(128, 128, image::ImageFormat::Png);

    let hash1 = run_phash(&wasm, &png);
    let hash2 = run_phash(&wasm, &png);

    assert_eq!(hash1, hash2, "同一入力で異なるハッシュが返されました");
    // ハッシュが全ビット0や全ビット1でないことも確認（退化していないこと）
    assert_ne!(hash1, 0, "ハッシュが全ビット0です");
    assert_ne!(hash1, u64::MAX, "ハッシュが全ビット1です");
}
