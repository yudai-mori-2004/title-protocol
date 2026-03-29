// SPDX-License-Identifier: Apache-2.0

//! # image-pdq 統合テスト
//!
//! コンパイル済み image-pdq.wasm を WasmRunner で実行し、
//! PDQ 256-bit知覚ハッシュの正確性を検証する。
//!
//! ## 前提条件
//! ```bash
//! cd wasm/image-pdq && cargo build --target wasm32-unknown-unknown --release
//! ```

use std::io::Cursor;

use title_wasm_host::WasmRunner;

const WASM_RELATIVE: &str =
    "../../wasm/image-pdq/target/wasm32-unknown-unknown/release/image_pdq.wasm";

fn load_pdq_wasm() -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/{WASM_RELATIVE}");
    std::fs::read(path).ok()
}

fn run_pdq(wasm: &[u8], image_bytes: &[u8], mime: &str) -> serde_json::Value {
    let runner = WasmRunner::new(500_000_000, 64 * 1024 * 1024);
    runner
        .execute(wasm, image_bytes, mime, None, "process")
        .expect("image-pdq WASM実行に失敗")
        .output
}

fn hamming_distance_256(a: &str, b: &str) -> u32 {
    let a_bytes = hex::decode(a).unwrap();
    let b_bytes = hex::decode(b).unwrap();
    assert_eq!(a_bytes.len(), 32);
    assert_eq!(b_bytes.len(), 32);
    let mut dist = 0u32;
    for i in 0..32 {
        dist += (a_bytes[i] ^ b_bytes[i]).count_ones();
    }
    dist
}

/// グラデーション画像を生成
fn create_gradient(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        let r = (x * 255 / width.max(1)) as u8;
        let g = (y * 255 / height.max(1)) as u8;
        let b = 128;
        image::Rgb([r, g, b])
    });
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, format).unwrap();
    buf.into_inner()
}

/// 単色画像を生成
fn create_solid(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([r, g, b]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[test]
fn test_pdq_basic_output() {
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let img = create_gradient(256, 256, image::ImageFormat::Jpeg);
    let result = run_pdq(&wasm, &img, "image/jpeg");

    eprintln!("PDQ result: {}", result);

    // Check fields
    let hash = result["pdqhash"].as_str().unwrap();
    assert_eq!(hash.len(), 64, "PDQ hash should be 64 hex chars (256 bits)");
    assert_eq!(result["algorithm"].as_str(), Some("pdq"));
    assert_eq!(result["bits"].as_u64(), Some(256));
    assert!(result["quality"].as_u64().is_some());
}

#[test]
fn test_pdq_deterministic() {
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let img = create_gradient(512, 512, image::ImageFormat::Jpeg);
    let r1 = run_pdq(&wasm, &img, "image/jpeg");
    let r2 = run_pdq(&wasm, &img, "image/jpeg");

    assert_eq!(
        r1["pdqhash"].as_str(),
        r2["pdqhash"].as_str(),
        "Same image should produce identical PDQ hash"
    );
}

#[test]
fn test_pdq_same_photo_reencoded() {
    // 合成グラデーション画像はJPEG圧縮に弱いため、実写真で検証する。
    // 同じPixel写真を別品質でJPEGに再エンコードして距離を測定。
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let original = match std::fs::read(format!(
        "{manifest_dir}/../../integration-tests/fixtures/images/jpeg/pixel_plane.jpg"
    )) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("SKIP: pixel_photo_plane.jpg not found");
            return;
        }
    };

    // 元画像をデコード→低品質JPEGに再エンコード
    let img = image::load_from_memory(&original).unwrap();
    let mut reencoded = std::io::Cursor::new(Vec::new());
    img.write_to(&mut reencoded, image::ImageFormat::Jpeg).unwrap();
    let reencoded = reencoded.into_inner();

    let r_orig = run_pdq(&wasm, &original, "image/jpeg");
    let r_re = run_pdq(&wasm, &reencoded, "image/jpeg");

    let h_orig = r_orig["pdqhash"].as_str().unwrap();
    let h_re = r_re["pdqhash"].as_str().unwrap();

    let dist = hamming_distance_256(h_orig, h_re);
    eprintln!(
        "Original vs Re-encoded: distance={} (orig={}, re={})",
        dist, h_orig, h_re
    );
    // PDQ threshold is 31. Re-encoded real photo should be well within.
    assert!(
        dist <= 31,
        "Re-encoded photo should match within PDQ threshold (31), got {}",
        dist
    );
}

#[test]
fn test_pdq_resize_robustness() {
    // 同じ実写真を異なる解像度でリサイズし、PDQハッシュが近いことを確認。
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let original = match std::fs::read(format!(
        "{manifest_dir}/../../integration-tests/fixtures/images/jpeg/pixel_plane.jpg"
    )) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("SKIP: pixel_photo_plane.jpg not found");
            return;
        }
    };

    // 元画像を1/4にリサイズ
    let img = image::load_from_memory(&original).unwrap();
    let (w, h) = (img.width(), img.height());
    let small = img.resize(w / 4, h / 4, image::imageops::FilterType::Lanczos3);
    let mut small_buf = std::io::Cursor::new(Vec::new());
    small.write_to(&mut small_buf, image::ImageFormat::Png).unwrap();
    let small_bytes = small_buf.into_inner();

    let r_orig = run_pdq(&wasm, &original, "image/jpeg");
    let r_small = run_pdq(&wasm, &small_bytes, "image/png");

    let h_orig = r_orig["pdqhash"].as_str().unwrap();
    let h_small = r_small["pdqhash"].as_str().unwrap();

    let dist = hamming_distance_256(h_orig, h_small);
    eprintln!(
        "Original ({}x{}) vs 1/4 ({}x{}): distance={}",
        w, h, w / 4, h / 4, dist
    );
    // リサイズされた実写真はPDQ閾値31以内であるべき
    assert!(
        dist <= 31,
        "Resized photo should match within PDQ threshold (31), got {}",
        dist
    );
}

#[test]
fn test_pdq_different_images() {
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let gradient = create_gradient(256, 256, image::ImageFormat::Png);
    let solid = create_solid(256, 256, 200, 50, 50);

    let r1 = run_pdq(&wasm, &gradient, "image/png");
    let r2 = run_pdq(&wasm, &solid, "image/png");

    let h1 = r1["pdqhash"].as_str().unwrap();
    let h2 = r2["pdqhash"].as_str().unwrap();

    let dist = hamming_distance_256(h1, h2);
    eprintln!(
        "Gradient vs Solid: distance={} (g={}, s={})",
        dist, h1, h2
    );
    assert!(
        dist > 31,
        "Different images should have high distance (> PDQ threshold 31), got {}",
        dist
    );
}

#[test]
fn test_pdq_quality_low_for_solid() {
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let solid = create_solid(256, 256, 128, 128, 128);
    let result = run_pdq(&wasm, &solid, "image/png");

    let quality = result["quality"].as_u64().unwrap();
    eprintln!("Solid gray quality: {}", quality);
    assert!(
        quality < 10,
        "Solid color image should have very low quality, got {}",
        quality
    );
}

#[test]
fn test_pdq_quality_high_for_detailed() {
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let gradient = create_gradient(256, 256, image::ImageFormat::Png);
    let result = run_pdq(&wasm, &gradient, "image/png");

    let quality = result["quality"].as_u64().unwrap();
    eprintln!("Gradient quality: {}", quality);
    assert!(
        quality > 10,
        "Detailed image should have meaningful quality, got {}",
        quality
    );
}

#[test]
fn test_pdq_pixel_photo() {
    let wasm = match load_pdq_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: image-pdq.wasm not found");
            return;
        }
    };

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let plane = match std::fs::read(format!(
        "{manifest_dir}/../../integration-tests/fixtures/images/jpeg/pixel_plane.jpg"
    )) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("SKIP: pixel_photo_plane.jpg not found");
            return;
        }
    };
    let ramen = match std::fs::read(format!(
        "{manifest_dir}/../../integration-tests/fixtures/images/jpeg/pixel_ramen.jpg"
    )) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("SKIP: pixel_photo_ramen.jpg not found");
            return;
        }
    };

    let r_plane = run_pdq(&wasm, &plane, "image/jpeg");
    let r_ramen = run_pdq(&wasm, &ramen, "image/jpeg");

    let h_plane = r_plane["pdqhash"].as_str().unwrap();
    let h_ramen = r_ramen["pdqhash"].as_str().unwrap();

    eprintln!("Plane:  hash={} quality={}", h_plane, r_plane["quality"]);
    eprintln!("Ramen:  hash={} quality={}", h_ramen, r_ramen["quality"]);

    let dist = hamming_distance_256(h_plane, h_ramen);
    eprintln!("Plane vs Ramen distance: {}", dist);

    // These are completely different photos — distance should be high
    assert!(
        dist > 31,
        "Different photos should exceed PDQ threshold (31), got {}",
        dist
    );

    // Both should have high quality (real photos)
    assert!(r_plane["quality"].as_u64().unwrap() > 20);
    assert!(r_ramen["quality"].as_u64().unwrap() > 20);
}
