// SPDX-License-Identifier: Apache-2.0

//! # cert-* 統合テスト
//!
//! コンパイル済み cert-google.wasm を WasmRunner で実行し、
//! Pixel写真のC2PA証明書チェーン検証を行う。
//!
//! ## 前提条件
//! ```bash
//! cd wasm/cert-google && cargo build --target wasm32-unknown-unknown --release
//! ```
//!
//! WASM バイナリが存在しない場合、テストはスキップされる。

use title_wasm_host::WasmRunner;

/// cert-google.wasm のパス
const CERT_GOOGLE_WASM: &str =
    "../../wasm/cert-google/target/wasm32-unknown-unknown/release/cert_google.wasm";

/// Pixel写真のパス
const PIXEL_PLANE: &str = "../../tests/fixtures/images/jpeg/pixel_plane.jpg";
const PIXEL_RAMEN: &str = "../../tests/fixtures/images/jpeg/pixel_ramen.jpg";

/// WASMバイナリをロードする。ビルドされていなければ None。
fn load_wasm(relative_path: &str) -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/{relative_path}");
    std::fs::read(path).ok()
}

/// Pixel写真をロードする。
fn load_fixture(relative_path: &str) -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/{relative_path}");
    std::fs::read(path).ok()
}

#[test]
fn test_cert_google_plane() {
    let wasm = match load_wasm(CERT_GOOGLE_WASM) {
        Some(w) => w,
        None => {
            eprintln!("cert-google.wasm not found, skipping test");
            return;
        }
    };
    let content = match load_fixture(PIXEL_PLANE) {
        Some(c) => c,
        None => {
            eprintln!("pixel_photo_plane.jpg not found, skipping test");
            return;
        }
    };

    let runner = WasmRunner::new(200_000_000, 64 * 1024 * 1024);
    let result = runner
        .execute(&wasm, &content, "image/jpeg", None, "process")
        .expect("cert-google WASM実行に失敗");

    eprintln!("cert-google result: {}", result.output);

    // verified == true
    assert_eq!(
        result.output["verified"].as_bool(),
        Some(true),
        "plane.jpg should verify against Google Root CA"
    );

    // certs_der should have 2 entries (leaf + intermediate), base64-encoded DER
    let certs = result.output["certs_der"].as_array().expect("certs_der should be array");
    assert_eq!(certs.len(), 2, "Should have 2 certs in chain");

    // Each entry should be a non-empty base64 string
    for (i, cert) in certs.iter().enumerate() {
        let b64 = cert.as_str().unwrap_or("");
        assert!(b64.len() > 100, "cert[{i}] should be substantial base64, got len={}", b64.len());
    }

    // root_ca should be present
    assert_eq!(
        result.output["root_ca"].as_str(),
        Some("Google C2PA Root CA G3")
    );

    // root_spki should be the full SPKI DER hex
    let spki = result.output["root_spki"].as_str().unwrap();
    assert!(spki.len() > 100, "root_spki should be full SPKI hex, got len={}", spki.len());
}

#[test]
fn test_cert_google_ramen() {
    let wasm = match load_wasm(CERT_GOOGLE_WASM) {
        Some(w) => w,
        None => {
            eprintln!("cert-google.wasm not found, skipping test");
            return;
        }
    };
    let content = match load_fixture(PIXEL_RAMEN) {
        Some(c) => c,
        None => {
            eprintln!("pixel_photo_ramen.jpg not found, skipping test");
            return;
        }
    };

    let runner = WasmRunner::new(200_000_000, 64 * 1024 * 1024);
    let result = runner
        .execute(&wasm, &content, "image/jpeg", None, "process")
        .expect("cert-google WASM実行に失敗");

    assert_eq!(
        result.output["verified"].as_bool(),
        Some(true),
        "ramen.jpg should verify against Google Root CA"
    );
}

#[test]
fn test_cert_google_non_c2pa_content() {
    let wasm = match load_wasm(CERT_GOOGLE_WASM) {
        Some(w) => w,
        None => {
            eprintln!("cert-google.wasm not found, skipping test");
            return;
        }
    };

    // Minimal JPEG without C2PA data
    let content = vec![0xFF, 0xD8, 0xFF, 0xD9];

    let runner = WasmRunner::new(200_000_000, 64 * 1024 * 1024);
    let result = runner
        .execute(&wasm, &content, "image/jpeg", None, "process")
        .expect("cert-google WASM実行に失敗");

    // Should return verified: false (no C2PA data)
    assert_eq!(
        result.output["verified"].as_bool(),
        Some(false),
        "Non-C2PA content should not verify"
    );
}
