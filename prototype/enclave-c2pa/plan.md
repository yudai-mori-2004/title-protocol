良い計画です。これで「TEEがネットワーク経由でデータを取得」「C2PA検証」「任意WASM実行」「外部RPC呼び出し」の全パスを一度にテストできます。

## アーキテクチャ

```
                          Internet
                             │
┌────────────────────────────┼──────────────────────┐
│  EC2 (Parent)              │                       │
│                            │                       │
│  ┌─────────┐    HTTP/S     │                       │
│  │  proxy   │◄─────────────┘                       │
│  │ (vsock   │  reqwest で R2, Solana RPC に接続     │
│  │  :8000)  │                                      │
│  └────┬─────┘                                      │
│       │ vsock                                      │
│  ┌────┴─────────────────────────────────────┐      │
│  │  Nitro Enclave                            │      │
│  │                                           │      │
│  │  1. vsock :5000 でコマンド受信             │      │
│  │  2. proxy経由で画像fetch (GET)            │      │
│  │  3. c2pa-rs で検証                        │      │
│  │  4. image crateでデコード → WASM実行      │      │
│  │  5. proxy経由でSolana RPC (POST)          │      │
│  │  6. 結果をまとめて返却                     │      │
│  └───────────────────────────────────────────┘      │
│                                                     │
│  parent-app: URL + WASM bytes を TEE に送信          │
└─────────────────────────────────────────────────────┘
```

## ディレクトリ構成

```bash
mkdir -p ~/enclave-c2pa-v2 && cd ~/enclave-c2pa-v2
```

```
enclave-c2pa-v2/
├── Cargo.toml
├── enclave/
│   ├── Cargo.toml
│   └── src/main.rs
├── proxy/
│   ├── Cargo.toml
│   └── src/main.rs
├── parent/
│   ├── Cargo.toml
│   └── src/main.rs
├── wasm-brightness/
│   ├── Cargo.toml
│   └── src/lib.rs
└── Dockerfile
```

## ファイル一覧

### workspace Cargo.toml

```toml
# Cargo.toml
[workspace]
members = ["enclave", "proxy", "parent", "wasm-brightness"]
resolver = "2"
```

### vsock HTTPプロキシ（親側で常駐）

```toml
# proxy/Cargo.toml
[package]
name = "vsock-proxy"
version = "0.1.0"
edition = "2021"

[dependencies]
vsock = "0.4"
reqwest = { version = "0.12", features = ["blocking", "rustls-tls"], default-features = false }
```

```rust
// proxy/src/main.rs
//
// TEEからのHTTPリクエストを代行するvsockプロキシ。
// TEEにはネットワークがないので、全ての外部通信はここを経由する。
//
// プロトコル (TEE → Proxy):
//   [4B: method_len][method][4B: url_len][url][4B: body_len][body]
//
// プロトコル (Proxy → TEE):
//   [4B: status_code][4B: body_len][body]

use std::io::{Read, Write};
use std::thread;

fn read_u32(stream: &mut impl Read) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

fn read_string(stream: &mut impl Read) -> std::io::Result<String> {
    let len = read_u32(stream)? as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn read_bytes(stream: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let len = read_u32(stream)? as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn handle_connection(mut stream: vsock::VsockStream) {
    let method = match read_string(&mut stream) {
        Ok(m) => m,
        Err(e) => { eprintln!("Proxy: failed to read method: {}", e); return; }
    };
    let url = match read_string(&mut stream) {
        Ok(u) => u,
        Err(e) => { eprintln!("Proxy: failed to read url: {}", e); return; }
    };
    let body = match read_bytes(&mut stream) {
        Ok(b) => b,
        Err(e) => { eprintln!("Proxy: failed to read body: {}", e); return; }
    };

    eprintln!("Proxy: {} {} (body: {} bytes)", method, url, body.len());

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap();

    let response = match method.as_str() {
        "GET" => client.get(&url).send(),
        "POST" => client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send(),
        _ => {
            eprintln!("Proxy: unsupported method: {}", method);
            return;
        }
    };

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16() as u32;
            let body_bytes = resp.bytes().unwrap_or_default().to_vec();
            eprintln!("Proxy: response status={}, body={} bytes", status, body_bytes.len());

            let _ = stream.write_all(&status.to_be_bytes());
            let _ = stream.write_all(&(body_bytes.len() as u32).to_be_bytes());
            let _ = stream.write_all(&body_bytes);
        }
        Err(e) => {
            eprintln!("Proxy: request failed: {}", e);
            let error_msg = format!("Proxy error: {}", e).into_bytes();
            let _ = stream.write_all(&500u32.to_be_bytes());
            let _ = stream.write_all(&(error_msg.len() as u32).to_be_bytes());
            let _ = stream.write_all(&error_msg);
        }
    }
}

fn main() {
    let port = 8000u32;
    eprintln!("Proxy: listening on vsock port {}", port);

    let listener = vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, port)
        .expect("Failed to bind vsock");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                thread::spawn(move || handle_connection(s));
            }
            Err(e) => eprintln!("Proxy: accept error: {}", e),
        }
    }
}
```

### WASMモジュール（平均輝度）

```toml
# wasm-brightness/Cargo.toml
[package]
name = "wasm-brightness"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]
```

```rust
// wasm-brightness/src/lib.rs
//
// TEEから渡されるRGBAピクセルデータの平均輝度を計算する。
// wasm32-unknown-unknown ターゲットでコンパイルする。
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// WASM線形メモリを拡張してポインタを返す
#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let pages_needed = ((size as usize) + 65535) / 65536;
    let old_pages = core::arch::wasm32::memory_grow(0, pages_needed);
    if old_pages == usize::MAX {
        return 0;
    }
    (old_pages * 65536) as u32
}

/// RGBAピクセルデータから知覚輝度の平均を計算
/// 知覚輝度 = 0.299R + 0.587G + 0.114B
#[no_mangle]
pub extern "C" fn compute_brightness(ptr: u32, len: u32) -> f32 {
    let data = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let pixel_count = (len as usize) / 4; // RGBA
    if pixel_count == 0 {
        return 0.0;
    }

    let mut sum: u64 = 0;
    for i in 0..pixel_count {
        let base = i * 4;
        let r = data[base] as u64;
        let g = data[base + 1] as u64;
        let b = data[base + 2] as u64;
        sum += (299 * r + 587 * g + 114 * b) / 1000;
    }

    (sum as f64 / pixel_count as f64) as f32
}
```

### Enclave本体

```toml
# enclave/Cargo.toml
[package]
name = "enclave-app"
version = "0.1.0"
edition = "2021"

[dependencies]
vsock = "0.4"
c2pa = "0.44"
image = { version = "0.25", default-features = false, features = ["jpeg", "png"] }
wasmtime = "29"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"
```

```rust
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
```

### 親側クライアント

```toml
# parent/Cargo.toml
[package]
name = "parent-app"
version = "0.1.0"
edition = "2021"

[dependencies]
vsock = "0.4"
base64 = "0.22"
```

```rust
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
```

parent/Cargo.tomlにserde_jsonを追加してください。

```toml
# parent/Cargo.toml に追加
serde_json = "1"
```

### Dockerfile

```dockerfile
FROM amazonlinux:2023 AS builder

RUN dnf install -y gcc gcc-c++ openssl-devel perl-FindBin perl-File-Compare make pkg-config cmake3

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

# workspace全体をコピー
COPY Cargo.toml ./
COPY enclave/ enclave/
COPY proxy/ proxy/
COPY parent/ parent/
COPY wasm-brightness/ wasm-brightness/

# enclaveバイナリのみビルド
RUN cargo build --release --package enclave-app

FROM amazonlinux:2023

# openssl実行時ライブラリ
RUN dnf install -y openssl

COPY --from=builder /app/target/release/enclave-app /usr/local/bin/enclave-app

CMD ["/usr/local/bin/enclave-app"]
```

## ビルド手順

### 1. WASMモジュールをビルド（EC2上）

```bash
cd ~/enclave-c2pa-v2

# wasm targetを追加
rustup target add wasm32-unknown-unknown

# ビルド
cargo build --release --package wasm-brightness --target wasm32-unknown-unknown

# 確認
ls -la target/wasm32-unknown-unknown/release/wasm_brightness.wasm
# 数KB〜数十KBのはず
```

### 2. proxy と parent をEC2上でビルド

```bash
cargo build --release --package vsock-proxy --package parent-app
```

### 3. Enclave用DockerイメージをビルドしてEIF化

```bash
docker build -t enclave-c2pa-v2 .

nitro-cli build-enclave \
  --docker-uri enclave-c2pa-v2:latest \
  --output-file enclave-c2pa-v2.eif
```

### 4. テスト用画像を用意

C2PA付き画像が理想ですが、まずは普通のJPEGでも動作確認できます（C2PAの結果が `no_manifest` になるだけ）。

R2やS3に画像を置いて、パブリックURLを取得してください。

C2PA付きテスト画像が欲しい場合は以下を参考に。

```bash
# c2patool でテスト画像を作る (EC2上)
cargo install c2patool
c2patool sample.jpg --manifest manifest.json --output test_c2pa.jpg
# manifest.json の例:
# {
#   "claim_generator": "test",
#   "assertions": [
#     { "label": "c2pa.actions", "data": { "actions": [{ "action": "c2pa.created" }] } }
#   ]
# }
```

## 実行手順

ターミナルを3つ用意します。

### ターミナル1: プロキシ起動

```bash
cd ~/enclave-c2pa-v2
./target/release/vsock-proxy
# → Proxy: listening on vsock port 8000
```

### ターミナル2: Enclave起動

```bash
# 既存のEnclaveを停止
nitro-cli terminate-enclave --all

# メモリを多めに（wasmtimeとimage crateが使う）
nitro-cli run-enclave \
  --eif-path enclave-c2pa-v2.eif \
  --cpu-count 2 \
  --memory 4096 \
  --enclave-cid 16 \
  --debug-mode

# ログ確認（別ターミナルでも可）
nitro-cli console --enclave-id $(nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')
```

### ターミナル3: テスト実行

```bash
cd ~/enclave-c2pa-v2

./target/release/parent-app \
  "https://your-r2-bucket.dev/test.jpg" \
  ./target/wasm32-unknown-unknown/release/wasm_brightness.wasm
```

### 期待される出力

```json
{
  "c2pa": {
    "status": "ok",
    "label": "urn:uuid:...",
    "claim_generator": "test",
    "ingredients_count": 0,
    "validation_status": []
  },
  "wasm": {
    "avg_brightness": 127.34,
    "pixel_count": 2073600,
    "rgba_bytes": 8294400
  },
  "solana": {
    "jsonrpc": "2.0",
    "id": 1,
    "result": 328456789
  },
  "image_bytes": 1234567
}
```

## トラブルが起きやすいポイント

**proxy接続失敗**: Enclaveからproxy（CID=3, port=8000）に繋がらない場合、proxyが起動していることを確認。`--debug-mode`のコンソールに `proxy connect failed` と出る。

**メモリ不足**: wasmtimeのコンパイルはメモリを食います。`--memory 4096` で足りなければ `8192` に上げてください。allocator.yamlの `memory_mib` も合わせて更新が必要です。

**HTTPS証明書**: Enclave内にはCA証明書がない場合があります。Dockerfileに `RUN dnf install -y ca-certificates` を追加してください。

**大きい画像**: 数十MBの画像だとvsock転送に時間がかかります。最初は1MB以下の画像でテストしてください。