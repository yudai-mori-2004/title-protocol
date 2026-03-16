use title_wasm_host::WasmRunner;

const WASM_RELATIVE: &str =
    "../../wasm/phash-v1/target/wasm32-unknown-unknown/release/phash_v1.wasm";

fn load_phash_wasm() -> Vec<u8> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/{WASM_RELATIVE}");
    std::fs::read(&path).unwrap_or_else(|_| panic!("WASM not found at {path}"))
}

#[test]
fn test_rootlens_image_phash() {
    let wasm = load_phash_wasm();
    let image_bytes = std::fs::read("/Users/forest/WebCreations/RootLens/web/public/c2pa_signed_1773548996967.jpg")
        .expect("image not found");

    let runner = WasmRunner::new(100_000_000, 64 * 1024 * 1024);
    let result = runner
        .execute(&wasm, &image_bytes, None, "process")
        .expect("phash failed");

    let hash_str = result.output["phash"].as_str().unwrap();
    eprintln!("Computed pHash: {hash_str}");
    eprintln!("On-chain pHash (orientation-corrected): ed86f21629dc9d09");

    let computed = u64::from_str_radix(hash_str, 16).unwrap();
    let onchain = u64::from_str_radix("ed86f21629dc9d09", 16).unwrap();
    let dist = (computed ^ onchain).count_ones();
    eprintln!("Hamming distance: {dist}");

    assert!(dist <= 5, "Distance {dist} too large");
}
