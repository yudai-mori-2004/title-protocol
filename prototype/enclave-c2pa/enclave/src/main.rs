// enclave/src/main.rs

use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};

// ───── vsock HTTPクライアント（proxy経由）─────

const PROXY_CID: u32 = 3; // 親インスタンス
const PROXY_PORT: u32 = 8000;

struct HttpResponse {
    status: u32,
    body: Vec<u8>,
}

fn http_request(method: &str, url: &str, body: &[u8]) -> Result<HttpResponse, String> {
    let mut stream = vsock::VsockStream::connect_with_cid_port(PROXY_CID, PROXY_PORT)
        .map_err(|e| format!("proxy connect failed: {}", e))?;

    // method
    let method_bytes = method.as_bytes();
    stream.write_all(&(method_bytes.len() as u32).to_be_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(method_bytes).map_err(|e| e.to_string())?;

    // url
    let url_bytes = url.as_bytes();
    stream.write_all(&(url_bytes.len() as u32).to_be_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(url_bytes).map_err(|e| e.to_string())?;

    // body
    stream.write_all(&(body.len() as u32).to_be_bytes()).map_err(|e| e.to_string())?;
    if !body.is_empty() {
        stream.write_all(body).map_err(|e| e.to_string())?;
    }

    // response
    let mut buf4 = [0u8; 4];
    stream.read_exact(&mut buf4).map_err(|e| e.to_string())?;
    let status = u32::from_be_bytes(buf4);

    stream.read_exact(&mut buf4).map_err(|e| e.to_string())?;
    let body_len = u32::from_be_bytes(buf4) as usize;

    let mut resp_body = vec![0u8; body_len];
    stream.read_exact(&mut resp_body).map_err(|e| e.to_string())?;

    Ok(HttpResponse { status, body: resp_body })
}

fn http_get(url: &str) -> Result<HttpResponse, String> {
    http_request("GET", url, &[])
}

fn http_post(url: &str, body: &[u8]) -> Result<HttpResponse, String> {
    http_request("POST", url, body)
}

// ───── C2PA検証 ─────

fn verify_c2pa(data: &[u8]) -> serde_json::Value {
    let reader = match c2pa::Reader::from_stream("image/jpeg", &mut Cursor::new(data)) {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({
                "status": "error",
                "error": format!("{}", e)
            });
        }
    };

    let active = match reader.active_manifest() {
        Some(m) => m,
        None => {
            return serde_json::json!({
                "status": "no_manifest",
            });
        }
    };

    let errors: Vec<String> = reader
        .validation_status()
        .map(|statuses| {
            statuses
                .iter()
                .map(|s| s.code().to_string())
                .collect()
        })
        .unwrap_or_default();

    serde_json::json!({
        "status": "ok",
        "label": active.label(),
        "title": active.title(),
        "claim_generator": active.claim_generator(),
        "ingredients_count": active.ingredients().len(),
        "validation_status": errors,
    })
}

// ───── WASM実行 ─────

fn run_wasm(wasm_bytes: &[u8], rgba_pixels: &[u8]) -> Result<serde_json::Value, String> {
    use wasmtime::*;

    let engine = Engine::default();
    let module = Module::new(&engine, wasm_bytes).map_err(|e| format!("wasm compile: {}", e))?;
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[])
        .map_err(|e| format!("wasm instantiate: {}", e))?;

    // エクスポート取得
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or("no memory export")?;
    let alloc_fn = instance
        .get_typed_func::<u32, u32>(&mut store, "alloc")
        .map_err(|e| format!("no alloc: {}", e))?;
    let compute_fn = instance
        .get_typed_func::<(u32, u32), f32>(&mut store, "compute_brightness")
        .map_err(|e| format!("no compute_brightness: {}", e))?;

    // WASM内にメモリ確保 → ピクセルデータをコピー
    let data_len = rgba_pixels.len() as u32;
    let ptr = alloc_fn
        .call(&mut store, data_len)
        .map_err(|e| format!("alloc failed: {}", e))?;
    if ptr == 0 {
        return Err("wasm alloc returned null".into());
    }

    memory
        .write(&mut store, ptr as usize, rgba_pixels)
        .map_err(|e| format!("memory write: {}", e))?;

    // 計算実行
    let brightness = compute_fn
        .call(&mut store, (ptr, data_len))
        .map_err(|e| format!("compute failed: {}", e))?;

    Ok(serde_json::json!({
        "avg_brightness": brightness,
        "pixel_count": rgba_pixels.len() / 4,
        "rgba_bytes": rgba_pixels.len(),
    }))
}

// ───── Solana RPC ─────

fn get_solana_block_height(rpc_url: &str) -> serde_json::Value {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBlockHeight"
    });
    let body_bytes = serde_json::to_vec(&body).unwrap();

    match http_post(rpc_url, &body_bytes) {
        Ok(resp) => {
            if resp.status == 200 {
                serde_json::from_slice(&resp.body).unwrap_or_else(|e| {
                    serde_json::json!({ "error": format!("json parse: {}", e) })
                })
            } else {
                serde_json::json!({
                    "error": format!("HTTP {}", resp.status),
                    "body": String::from_utf8_lossy(&resp.body).to_string()
                })
            }
        }
        Err(e) => serde_json::json!({ "error": e }),
    }
}

// ───── コマンドプロトコル ─────

#[derive(Deserialize)]
struct Command {
    image_url: String,
    wasm_b64: String, // base64エンコードされたWASMバイナリ
    solana_rpc_url: String,
}

#[derive(Serialize)]
struct FullResult {
    c2pa: serde_json::Value,
    wasm: serde_json::Value,
    solana: serde_json::Value,
    image_bytes: usize,
}

// ───── メイン ─────

fn main() {
    eprintln!("Enclave: starting on vsock port 5000");
    eprintln!("Enclave: proxy expected at CID={} port={}", PROXY_CID, PROXY_PORT);

    let listener = vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, 5000)
        .expect("Failed to bind vsock");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                eprintln!("Enclave: client connected");

                // コマンドJSON受信 (length-prefixed)
                let mut len_buf = [0u8; 4];
                if let Err(e) = stream.read_exact(&mut len_buf) {
                    eprintln!("Enclave: read len error: {}", e);
                    continue;
                }
                let cmd_len = u32::from_be_bytes(len_buf) as usize;

                let mut cmd_buf = vec![0u8; cmd_len];
                if let Err(e) = stream.read_exact(&mut cmd_buf) {
                    eprintln!("Enclave: read cmd error: {}", e);
                    continue;
                }

                let cmd: Command = match serde_json::from_slice(&cmd_buf) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Enclave: invalid command: {}", e);
                        continue;
                    }
                };

                eprintln!("Enclave: fetching image from {}", cmd.image_url);

                // ── 1. 画像取得 ──
                let image_data = match http_get(&cmd.image_url) {
                    Ok(resp) if resp.status == 200 => {
                        eprintln!("Enclave: fetched {} bytes", resp.body.len());
                        resp.body
                    }
                    Ok(resp) => {
                        eprintln!("Enclave: fetch failed with HTTP {}", resp.status);
                        continue;
                    }
                    Err(e) => {
                        eprintln!("Enclave: fetch error: {}", e);
                        continue;
                    }
                };

                // ── 2. C2PA検証 ──
                eprintln!("Enclave: running C2PA verification...");
                let c2pa_result = verify_c2pa(&image_data);
                eprintln!("Enclave: C2PA done: {}", c2pa_result["status"]);

                // ── 3. WASM実行 ──
                eprintln!("Enclave: decoding image for WASM...");
                let wasm_result = match image::load_from_memory(&image_data) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let pixels = rgba.as_raw();
                        eprintln!(
                            "Enclave: image {}x{}, {} RGBA bytes",
                            rgba.width(),
                            rgba.height(),
                            pixels.len()
                        );

                        match base64::Engine::decode(
                            &base64::engine::general_purpose::STANDARD,
                            &cmd.wasm_b64,
                        ) {
                            Ok(wasm_bytes) => {
                                eprintln!("Enclave: WASM binary {} bytes", wasm_bytes.len());
                                match run_wasm(&wasm_bytes, pixels) {
                                    Ok(v) => v,
                                    Err(e) => serde_json::json!({ "error": e }),
                                }
                            }
                            Err(e) => serde_json::json!({ "error": format!("base64: {}", e) }),
                        }
                    }
                    Err(e) => serde_json::json!({ "error": format!("image decode: {}", e) }),
                };
                eprintln!("Enclave: WASM done: {}", wasm_result);

                // ── 4. Solana RPC ──
                eprintln!("Enclave: calling Solana RPC...");
                let solana_result = get_solana_block_height(&cmd.solana_rpc_url);
                eprintln!("Enclave: Solana done: {}", solana_result);

                // ── 5. 結果を返却 ──
                let full = FullResult {
                    c2pa: c2pa_result,
                    wasm: wasm_result,
                    solana: solana_result,
                    image_bytes: image_data.len(),
                };
                let result_bytes = serde_json::to_vec_pretty(&full).unwrap();

                let _ = stream.write_all(&(result_bytes.len() as u32).to_be_bytes());
                let _ = stream.write_all(&result_bytes);
                eprintln!("Enclave: response sent ({} bytes)", result_bytes.len());
            }
            Err(e) => eprintln!("Enclave: accept error: {}", e),
        }
    }
}