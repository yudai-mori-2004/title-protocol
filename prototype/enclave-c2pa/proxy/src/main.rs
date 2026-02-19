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