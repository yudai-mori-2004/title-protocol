use std::io::{Read, Write};

fn main() {
    println!("Hello from inside Nitro Enclave!");

    // vsockでリッスン（CID=任意、Port=5000）
    // CID 3 = 親インスタンスからの接続を受け付ける
    let listener = vsock::VsockListener::bind_with_cid_port(
        vsock::VMADDR_CID_ANY, 5000
    ).expect("Failed to bind vsock");

    println!("Listening on vsock port 5000...");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).unwrap();
                let msg = String::from_utf8_lossy(&buf[..n]);
                println!("Received: {}", msg);

                let reply = format!("Echo from Enclave: {}", msg);
                stream.write_all(reply.as_bytes()).unwrap();
            }
            Err(e) => eprintln!("Connection error: {}", e),
        }
    }
}