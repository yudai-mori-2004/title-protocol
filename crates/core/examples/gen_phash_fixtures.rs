// SPDX-License-Identifier: Apache-2.0

//! pHashテスト用フィクスチャ生成ツール
//!
//! 指定ディレクトリ内の画像をC2PA署名し、signed/ サブディレクトリに出力する。
//! TSA (RFC 3161) タイムスタンプ付き。
//!
//! 使い方:
//!   cargo run --example gen_phash_fixtures -- <fixtures_dir> [--tsa <url>]
//!
//! 例:
//!   cargo run --example gen_phash_fixtures -- integration-tests/fixtures/phash-test
//!   cargo run --example gen_phash_fixtures -- integration-tests/fixtures/phash-test \
//!     --tsa http://timestamp.digicert.com

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const CERTS: &[u8] = include_bytes!("../../../tests/fixtures/certs/chain.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../tests/fixtures/certs/ee.key");

fn sign_image(
    source_bytes: &[u8],
    format: &str,
    title: &str,
    tsa_url: Option<String>,
) -> Vec<u8> {
    let signer =
        c2pa::create_signer::from_keys(CERTS, PRIVATE_KEY, c2pa::SigningAlg::Ed25519, tsa_url)
            .unwrap();

    let manifest_json = serde_json::json!({
        "title": title,
        "format": format,
        "claim_generator_info": [{
            "name": "title-phash-fixture-gen",
            "version": "0.1.0"
        }]
    })
    .to_string();

    let mut builder = c2pa::Builder::from_json(&manifest_json).unwrap();
    let mut source = Cursor::new(source_bytes);
    let mut dest = Cursor::new(Vec::new());
    builder
        .sign(signer.as_ref(), format, &mut source, &mut dest)
        .unwrap();
    dest.into_inner()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: gen_phash_fixtures <fixtures_dir> [--tsa <url>]");
        std::process::exit(1);
    }

    let fixtures_dir = PathBuf::from(&args[1]);
    let tsa_url = args
        .iter()
        .position(|a| a == "--tsa")
        .map(|i| args[i + 1].clone());

    if let Some(ref url) = tsa_url {
        println!("TSA URL: {}", url);
    }

    let signed_dir = fixtures_dir.join("signed");
    fs::create_dir_all(&signed_dir).unwrap();

    // 署名対象ファイルを収集（"original" を最初に署名 → TSA最古にする）
    let mut entries: Vec<_> = fs::read_dir(&fixtures_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            path.is_file()
                && !name.starts_with('.')
                && (name.ends_with(".jpg")
                    || name.ends_with(".jpeg")
                    || name.ends_with(".png"))
        })
        .collect();
    entries.sort_by_key(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        if name.contains("original") {
            (0, name)
        } else {
            (1, name)
        }
    });

    println!(
        "Found {} images in {}",
        entries.len(),
        fixtures_dir.display()
    );

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let source_bytes = fs::read(entry.path()).unwrap();

        let format = if name.ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        };

        println!(
            "  Signing: {} ({} KB, {})...",
            name,
            source_bytes.len() / 1024,
            format
        );

        let signed = sign_image(&source_bytes, format, &name, tsa_url.clone());

        let out_path = signed_dir.join(&name);
        fs::write(&out_path, &signed).unwrap();
        println!(
            "    → {} ({} KB)",
            out_path.display(),
            signed.len() / 1024
        );

        // TSA署名間に少し待つ（タイムスタンプが異なるように）
        if tsa_url.is_some() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    println!("\nAll phash fixtures signed in: {}", signed_dir.display());
}
