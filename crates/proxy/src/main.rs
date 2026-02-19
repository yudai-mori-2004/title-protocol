//! # Title Protocol vsock HTTPプロキシ
//!
//! prototypeのenclave-c2pa/proxy/をベースに、tokio非同期化した実装。
//!
//! TEEにはネットワークアクセスがないため、全ての外部HTTP通信は
//! このプロキシを経由する。
//!
//! ## プロトコル (TEE → Proxy)
//! ```text
//! [4B: method_len][method][4B: url_len][url][4B: body_len][body]
//! ```
//!
//! ## プロトコル (Proxy → TEE)
//! ```text
//! [4B: status_code][4B: body_len][body]
//! ```
//!
//! ## 変更点 (prototypeからの改善)
//! - `std::thread::spawn` → `tokio::spawn` による非同期並行処理
//! - `reqwest::blocking` → `reqwest` 非同期クライアント
//!
//! ## プラットフォーム
//! vsockはLinux専用。macOSではスタブエントリポイントのみ提供。

/// vsockリッスンポート
const VSOCK_PORT: u32 = 8000;

/// vsock接続を処理する（Linux専用）。
///
/// prototypeのenclave-c2pa/proxy/src/main.rsと同一のlength-prefixedプロトコルを使用。
#[cfg(target_os = "linux")]
async fn handle_connection(_stream: vsock::VsockStream) {
    // TODO: read_string でメソッドを読み取り
    // TODO: read_string でURLを読み取り
    // TODO: read_bytes でボディを読み取り
    // TODO: reqwest 非同期クライアントで外部HTTP通信
    // TODO: レスポンスを [status_code][body_len][body] 形式で書き戻し
    todo!("vsock接続のハンドリング（非同期版）")
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    tracing::info!("vsock HTTPプロキシを port {} で起動します", VSOCK_PORT);

    let listener = vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, VSOCK_PORT)
        .expect("vsockのバインドに失敗しました");

    // tokio::spawnで並行処理（prototypeのthread::spawnからの改善）
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                tokio::spawn(async move {
                    handle_connection(s).await;
                });
            }
            Err(e) => {
                tracing::error!("vsock accept エラー: {}", e);
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::warn!(
        "vsock HTTPプロキシはLinux専用です。macOS/Windowsでは動作しません。port={}",
        VSOCK_PORT
    );
    Ok(())
}
