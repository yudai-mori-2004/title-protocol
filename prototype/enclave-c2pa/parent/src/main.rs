// parent/src/main.rs
use base64::Engine;
use std::env;
use std::fs;
use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: parent-app <image-url> <wasm-file-path>");
        eprintln!("");
        eprintln!("Example:");
        eprintln!("  parent-app https://your-r2.dev/test.jpg ./wasm_brightness.wasm");
        std::process::exit(1);
    }

    let image_url = &args[1];
    let wasm_path = &args[2];

    // Solana mainnet RPC (public, rate-limited but sufficient for test)
    let solana_rpc = "https://api.mainnet-beta.solana.com";

    // WASMバイナリを読み込んでbase64エンコード
    let wasm_bytes = fs::read(wasm_path).expect("Failed to read WASM file");
    let wasm_b64 = base64::engine::general_purpose::STANDARD.encode(&wasm_bytes);
    println!("WASM: {} bytes -> base64 {} bytes", wasm_bytes.len(), wasm_b64.len());

    // コマンドJSON構築
    let cmd = serde_json::json!({
        "image_url": image_url,
        "wasm_b64": wasm_b64,
        "solana_rpc_url": solana_rpc,
    });
    let cmd_bytes = serde_json::to_vec(&cmd).unwrap();
    println!("Command payload: {} bytes", cmd_bytes.len());

    // Enclaveに接続
    let enclave_cid = 16u32;
    println!("Connecting to Enclave (CID={})...", enclave_cid);
    let mut stream = vsock::VsockStream::connect_with_cid_port(enclave_cid, 5000)
        .expect("Failed to connect to enclave");
    println!("Connected!");

    // 送信
    stream
        .write_all(&(cmd_bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&cmd_bytes).unwrap();
    println!("Sent command. Waiting for result...");

    // 結果受信
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).unwrap();
    let result_len = u32::from_be_bytes(len_buf) as usize;

    let mut result_buf = vec![0u8; result_len];
    stream.read_exact(&mut result_buf).unwrap();

    let result: serde_json::Value = serde_json::from_slice(&result_buf).unwrap();
    println!("\n========== RESULT ==========");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}