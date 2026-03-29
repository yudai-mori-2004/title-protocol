// SPDX-License-Identifier: Apache-2.0

//! C2PA test fixture factory.
//!
//! Generates C2PA-signed and unsigned content for every format the c2pa crate
//! supports. Output structure:
//!
//!   tests/fixtures/c2pa/signed/   — C2PA-signed (factory-generated)
//!   tests/fixtures/c2pa/unsigned/ — bare media (no C2PA manifests)
//!   tests/fixtures/minimal/       — source files for factory input
//!
//! Usage:
//!   cargo run --example gen_c2pa_fixtures
//!
//! External fixtures (Google Pixel etc.) are NOT generated here.

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

const CERTS: &[u8] = include_bytes!("../../../tests/fixtures/certs/chain.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../tests/fixtures/certs/ee.key");
const MINIMAL_JPEG: &[u8] = include_bytes!("../../../tests/fixtures/minimal/test_4x4.jpg");
const MINIMAL_PNG: &[u8] = include_bytes!("../../../tests/fixtures/minimal/test_2x2.png");

fn test_signer() -> Box<dyn c2pa::Signer> {
    c2pa::create_signer::from_keys(CERTS, PRIVATE_KEY, c2pa::SigningAlg::Ed25519, None).unwrap()
}

fn sign(title: &str, format: &str, source: &[u8]) -> Vec<u8> {
    let json = serde_json::json!({
        "title": title, "format": format,
        "claim_generator_info": [{"name": "title-fixture-factory", "version": "0.1.0"}]
    }).to_string();
    let mut builder = c2pa::Builder::from_json(&json).unwrap();
    let mut src = Cursor::new(source);
    let mut dest = Cursor::new(Vec::new());
    builder.sign(test_signer().as_ref(), format, &mut src, &mut dest).unwrap();
    dest.into_inner()
}

fn sign_with_ingredients(title: &str, format: &str, source: &[u8], ingredients: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let json = serde_json::json!({
        "title": title, "format": format,
        "claim_generator_info": [{"name": "title-fixture-factory", "version": "0.1.0"}]
    }).to_string();
    let mut builder = c2pa::Builder::from_json(&json).unwrap();
    for (t, f, b) in ingredients {
        let ij = serde_json::json!({"title": t, "relationship": "inputTo"}).to_string();
        builder.add_ingredient_from_stream(&ij, f, &mut Cursor::new(b)).unwrap();
    }
    let mut src = Cursor::new(source);
    let mut dest = Cursor::new(Vec::new());
    builder.sign(test_signer().as_ref(), format, &mut src, &mut dest).unwrap();
    dest.into_inner()
}

// ---------------------------------------------------------------------------
// Minimal source file generators
// ---------------------------------------------------------------------------

fn png_to_tiff(png: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(png).unwrap();
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Tiff).unwrap();
    buf.into_inner()
}

fn generate_wav(sample_rate: u32, duration_secs: u32, frequency: f64) -> Vec<u8> {
    let num_samples = sample_rate * duration_secs;
    let bits: u16 = 16;
    let channels: u16 = 1;
    let byte_rate = sample_rate * (bits as u32 / 8) * channels as u32;
    let block_align = channels * (bits / 8);
    let data_size = num_samples * (bits as u32 / 8) * channels as u32;

    let mut wav = Vec::with_capacity(44 + data_size as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let sample = (t * frequency * 2.0 * std::f64::consts::PI).sin() * 16000.0;
        wav.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    wav
}

fn ffmpeg_generate(args: &[&str], output: &Path) -> Vec<u8> {
    let mut cmd_args = vec!["-y"];
    cmd_args.extend_from_slice(args);
    let status = Command::new("ffmpeg")
        .args(&cmd_args)
        .arg(output)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ffmpeg required");
    assert!(status.success(), "ffmpeg failed for {}", output.display());
    let data = fs::read(output).unwrap();
    let _ = fs::remove_file(output);
    data
}

fn generate_mp4(duration: u32, width: u32, height: u32) -> Vec<u8> {
    let tmp = std::env::temp_dir().join("title-factory.mp4");
    ffmpeg_generate(&[
        "-f", "lavfi", "-i",
        &format!("testsrc=duration={duration}:size={width}x{height}:rate=30"),
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
    ], &tmp)
}

fn generate_mp4_with_audio(duration: u32, width: u32, height: u32) -> Vec<u8> {
    let tmp = std::env::temp_dir().join("title-factory-av.mp4");
    ffmpeg_generate(&[
        "-f", "lavfi", "-i",
        &format!("testsrc=duration={duration}:size={width}x{height}:rate=30"),
        "-f", "lavfi", "-i",
        &format!("sine=frequency=440:duration={duration}:sample_rate=44100"),
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac",
        "-movflags", "+faststart",
    ], &tmp)
}

fn generate_mp3(duration: u32) -> Vec<u8> {
    let tmp = std::env::temp_dir().join("title-factory.mp3");
    ffmpeg_generate(&[
        "-f", "lavfi", "-i",
        &format!("sine=frequency=440:duration={duration}:sample_rate=44100"),
        "-c:a", "libmp3lame", "-b:a", "128k",
    ], &tmp)
}

fn generate_jpeg(width: u32, height: u32, pattern: &str) -> Vec<u8> {
    let tmp = std::env::temp_dir().join("title-factory.jpg");
    ffmpeg_generate(&[
        "-f", "lavfi", "-i",
        &format!("{pattern}=size={width}x{height}:rate=1"),
        "-frames:v", "1", "-q:v", "5",
    ], &tmp)
}

fn emit(dir: &Path, name: &str, data: &[u8]) {
    fs::write(dir.join(name), data).unwrap();
    println!("  {name} ({} bytes)", data.len());
}

fn main() {
    let signed = Path::new("tests/fixtures/c2pa/signed");
    let unsigned = Path::new("tests/fixtures/c2pa/unsigned");
    let minimal = Path::new("tests/fixtures/minimal");

    // Clean and recreate
    let _ = fs::remove_dir_all(signed);
    let _ = fs::remove_dir_all(unsigned);
    fs::create_dir_all(signed).unwrap();
    fs::create_dir_all(unsigned).unwrap();
    fs::create_dir_all(minimal).unwrap();

    // =====================================================================
    // Phase 1: Generate minimal source files
    // =====================================================================
    println!("=== Minimal source files ===");

    let tiff = png_to_tiff(MINIMAL_PNG);
    emit(minimal, "test.tiff", &tiff);

    let wav_short = generate_wav(8000, 1, 440.0);
    emit(minimal, "test.wav", &wav_short);

    let wav_long = generate_wav(44100, 5, 440.0);
    emit(minimal, "test_5s.wav", &wav_long);

    let mp4_tiny = generate_mp4(1, 64, 64);
    emit(minimal, "test.mp4", &mp4_tiny);

    let mp4_realistic = generate_mp4_with_audio(5, 640, 480);
    emit(minimal, "test_5s_640x480.mp4", &mp4_realistic);

    let mp4_long = generate_mp4_with_audio(10, 1280, 720);
    emit(minimal, "test_10s_720p.mp4", &mp4_long);

    let mp3 = generate_mp3(3);
    emit(minimal, "test.mp3", &mp3);

    let jpeg_640 = generate_jpeg(640, 480, "testsrc");
    emit(minimal, "test_640x480.jpg", &jpeg_640);

    let jpeg_1080 = generate_jpeg(1920, 1080, "mandelbrot");
    emit(minimal, "test_1080p.jpg", &jpeg_1080);

    // =====================================================================
    // Phase 2: C2PA-signed fixtures (all format × size combinations)
    // =====================================================================
    println!("\n=== C2PA-signed: images ===");

    // JPEG — multiple sizes
    emit(signed, "sample.jpg", &sign("sample.jpg", "image/jpeg", MINIMAL_JPEG));
    emit(signed, "jpeg-640x480.jpg", &sign("jpeg-640x480.jpg", "image/jpeg", &jpeg_640));
    emit(signed, "jpeg-1080p.jpg", &sign("jpeg-1080p.jpg", "image/jpeg", &jpeg_1080));

    // PNG
    emit(signed, "sample.png", &sign("sample.png", "image/png", MINIMAL_PNG));

    // TIFF
    emit(signed, "sample.tiff", &sign("sample.tiff", "image/tiff", &tiff));

    println!("\n=== C2PA-signed: video ===");

    // MP4 — multiple durations and sizes
    emit(signed, "mp4-1s-64x64.mp4", &sign("mp4-1s-64x64.mp4", "video/mp4", &mp4_tiny));
    emit(signed, "mp4-5s-640x480.mp4", &sign("mp4-5s-640x480.mp4", "video/mp4", &mp4_realistic));
    emit(signed, "mp4-10s-720p.mp4", &sign("mp4-10s-720p.mp4", "video/mp4", &mp4_long));

    println!("\n=== C2PA-signed: audio ===");

    // WAV
    emit(signed, "sample.wav", &sign("sample.wav", "audio/wav", &wav_short));
    emit(signed, "wav-5s.wav", &sign("wav-5s.wav", "audio/wav", &wav_long));

    // MP3
    emit(signed, "sample.mp3", &sign("sample.mp3", "audio/mpeg", &mp3));

    println!("\n=== C2PA-signed: provenance (ingredients) ===");

    // Single-ingredient
    let ing_a = sign("ingredient-a.jpg", "image/jpeg", MINIMAL_JPEG);
    emit(signed, "ingredient-a.jpg", &ing_a);

    let ing_b = sign("ingredient-b.jpg", "image/jpeg", MINIMAL_JPEG);
    emit(signed, "ingredient-b.jpg", &ing_b);

    // Two ingredients
    let with_2 = sign_with_ingredients("with-2-ingredients.jpg", "image/jpeg", MINIMAL_JPEG, &[
        ("ingredient-a.jpg", "image/jpeg", &ing_a),
        ("ingredient-b.jpg", "image/jpeg", &ing_b),
    ]);
    emit(signed, "with-2-ingredients.jpg", &with_2);

    // Three ingredients (deeper chain: C uses A+B, D uses C)
    let ing_c = sign_with_ingredients("ingredient-c.jpg", "image/jpeg", MINIMAL_JPEG, &[
        ("ingredient-a.jpg", "image/jpeg", &ing_a),
        ("ingredient-b.jpg", "image/jpeg", &ing_b),
    ]);
    emit(signed, "ingredient-c.jpg", &ing_c);

    let with_chain = sign_with_ingredients("with-chain.jpg", "image/jpeg", MINIMAL_JPEG, &[
        ("ingredient-c.jpg", "image/jpeg", &ing_c),
    ]);
    emit(signed, "with-chain.jpg", &with_chain);

    // =====================================================================
    // Phase 3: Unsigned fixtures (same formats, no C2PA)
    // =====================================================================
    println!("\n=== Unsigned fixtures ===");

    emit(unsigned, "sample.jpg", MINIMAL_JPEG);
    emit(unsigned, "sample.png", MINIMAL_PNG);
    emit(unsigned, "sample.tiff", &tiff);
    emit(unsigned, "sample.wav", &wav_short);
    emit(unsigned, "sample.mp4", &mp4_tiny);
    emit(unsigned, "sample.mp3", &mp3);
    emit(unsigned, "jpeg-1080p.jpg", &jpeg_1080);
    emit(unsigned, "mp4-5s-640x480.mp4", &mp4_realistic);

    println!("\n=== Summary ===");
    let signed_count = fs::read_dir(signed).unwrap().count();
    let unsigned_count = fs::read_dir(unsigned).unwrap().count();
    println!("Signed:   {} files in {}", signed_count, signed.display());
    println!("Unsigned: {} files in {}", unsigned_count, unsigned.display());
    println!("\nExternal fixtures (Google Pixel etc.) are under tests/fixtures/images/");
}
