//! C2PAテストフィクスチャ生成ツール
//!
//! E2Eテスト用のC2PA署名済みテスト画像を生成する。
//! setup-local.sh から呼び出される。
//!
//! 使い方:
//!   cargo run --example gen_fixture -- <output_dir>
//!
//! 生成されるファイル:
//!   - signed.jpg: 基本的なC2PA署名済みJPEG
//!   - ingredient_a.jpg: 素材用C2PA署名済みJPEG (A)
//!   - ingredient_b.jpg: 素材用C2PA署名済みJPEG (B)
//!   - with_ingredients.jpg: A, Bをingredientとして含むC2PA署名済みJPEG

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const CERTS: &[u8] = include_bytes!("../tests/fixtures/certs/chain.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../tests/fixtures/certs/ee.key");
const TEST_IMAGE: &[u8] = include_bytes!("../tests/fixtures/test.jpg");

fn test_signer() -> Box<dyn c2pa::Signer> {
    c2pa::create_signer::from_keys(CERTS, PRIVATE_KEY, c2pa::SigningAlg::Ed25519, None).unwrap()
}

fn create_signed(title: &str) -> Vec<u8> {
    let manifest_json = serde_json::json!({
        "title": title,
        "format": "image/jpeg",
        "claim_generator_info": [{
            "name": "title-fixture-gen",
            "version": "0.1.0"
        }]
    })
    .to_string();

    let mut builder = c2pa::Builder::from_json(&manifest_json).unwrap();
    let signer = test_signer();
    let mut source = Cursor::new(TEST_IMAGE);
    let mut dest = Cursor::new(Vec::new());
    builder
        .sign(signer.as_ref(), "image/jpeg", &mut source, &mut dest)
        .unwrap();
    dest.into_inner()
}

fn create_signed_with_ingredients(title: &str, ingredients: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest_json = serde_json::json!({
        "title": title,
        "format": "image/jpeg",
        "claim_generator_info": [{
            "name": "title-fixture-gen",
            "version": "0.1.0"
        }]
    })
    .to_string();

    let mut builder = c2pa::Builder::from_json(&manifest_json).unwrap();

    for (name, bytes) in ingredients {
        let ingredient_json = serde_json::json!({
            "title": name,
            "relationship": "inputTo"
        })
        .to_string();
        builder
            .add_ingredient_from_stream(
                &ingredient_json,
                "image/jpeg",
                &mut Cursor::new(bytes),
            )
            .unwrap();
    }

    let signer = test_signer();
    let mut source = Cursor::new(TEST_IMAGE);
    let mut dest = Cursor::new(Vec::new());
    builder
        .sign(signer.as_ref(), "image/jpeg", &mut source, &mut dest)
        .unwrap();
    dest.into_inner()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let output_dir = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from("tests/e2e/fixtures")
    };

    fs::create_dir_all(&output_dir).unwrap();

    // 1. 基本的なC2PA署名済みJPEG
    let signed = create_signed("signed.jpg");
    fs::write(output_dir.join("signed.jpg"), &signed).unwrap();
    println!("Generated: signed.jpg ({} bytes)", signed.len());

    // 2. 素材用A
    let ingredient_a = create_signed("ingredient_a.jpg");
    fs::write(output_dir.join("ingredient_a.jpg"), &ingredient_a).unwrap();
    println!("Generated: ingredient_a.jpg ({} bytes)", ingredient_a.len());

    // 3. 素材用B
    let ingredient_b = create_signed("ingredient_b.jpg");
    fs::write(output_dir.join("ingredient_b.jpg"), &ingredient_b).unwrap();
    println!("Generated: ingredient_b.jpg ({} bytes)", ingredient_b.len());

    // 4. A, Bをingredientとして含むJPEG
    let with_ingredients = create_signed_with_ingredients(
        "with_ingredients.jpg",
        &[
            ("ingredient_a.jpg", &ingredient_a),
            ("ingredient_b.jpg", &ingredient_b),
        ],
    );
    fs::write(output_dir.join("with_ingredients.jpg"), &with_ingredients).unwrap();
    println!(
        "Generated: with_ingredients.jpg ({} bytes)",
        with_ingredients.len()
    );

    println!("All fixtures generated in: {}", output_dir.display());
}
