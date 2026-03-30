// SPDX-License-Identifier: Apache-2.0
//! Sign a single file with C2PA test certificate.
//! Usage: cargo run --example sign_one -- <input> <output> <mime_type>

use std::io::Cursor;

const CERTS: &[u8] = include_bytes!("../../../tests/fixtures/certs/chain.pem");
const KEY: &[u8] = include_bytes!("../../../tests/fixtures/certs/ee.key");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: sign_one <input> <output> <mime_type>");
        std::process::exit(1);
    }
    let input = std::fs::read(&args[1]).expect("read input");
    let format = &args[3];

    let signer =
        c2pa::create_signer::from_keys(CERTS, KEY, c2pa::SigningAlg::Ed25519, None).unwrap();
    let json = serde_json::json!({
        "title": args[2].split('/').last().unwrap_or("signed"),
        "format": format,
        "claim_generator_info": [{"name": "title-sign-one", "version": "0.1.0"}]
    })
    .to_string();

    let mut builder = c2pa::Builder::from_json(&json).unwrap();
    let mut src = Cursor::new(&input);
    let mut dest = Cursor::new(Vec::new());
    builder
        .sign(signer.as_ref(), format, &mut src, &mut dest)
        .unwrap();

    std::fs::write(&args[2], dest.into_inner()).expect("write output");
}
