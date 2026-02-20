//! # Title Protocol vsock HTTPプロキシ
//!
//! `prototype/enclave-c2pa/proxy/` をベースに、tokio非同期化した実装。
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
//! - vsock acceptループを専用スレッド + mpscチャネルで分離
//!
//! ## プラットフォーム
//! - Linux: vsock port 8000 でリッスン
//! - macOS/Windows: TCP `127.0.0.1:8000` でリッスン（テスト用フォールバック）

/// vsockリッスンポート
#[cfg(target_os = "linux")]
const VSOCK_PORT: u32 = 8000;

/// TCPフォールバックアドレス（macOS/テスト用）
#[cfg(not(target_os = "linux"))]
const TCP_ADDR: &str = "127.0.0.1:8000";

// ============================================================
// 非同期プロトコルI/O（TCP経路: macOS / テスト）
// ============================================================

/// 仕様書 §6.4 length-prefixed protocol
/// ストリームから4バイトビッグエンディアンのu32を読み取る。
#[cfg(any(not(target_os = "linux"), test))]
async fn read_u32_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<u32> {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).await?;
    Ok(u32::from_be_bytes(buf))
}

/// length-prefixed文字列を読み取る。
#[cfg(any(not(target_os = "linux"), test))]
async fn read_string_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<String> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// length-prefixedバイト列を読み取る。
#[cfg(any(not(target_os = "linux"), test))]
async fn read_bytes_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

/// プロキシレスポンスを書き込む: [4B: status][4B: body_len][body]
#[cfg(any(not(target_os = "linux"), test))]
async fn write_response_async<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    status: u32,
    body: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    w.write_all(&status.to_be_bytes()).await?;
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await?;
    Ok(())
}

// ============================================================
// HTTP転送（共通ロジック）
// ============================================================

/// 仕様書 §6.4
/// TEEから受け取ったHTTPリクエストを外部に転送し、レスポンスを返す。
///
/// prototypeと同一のHTTPメソッドサポート（GET, POST）。
/// 未サポートのメソッドはステータス400を返す。
async fn forward_http(method: &str, url: &str, body: &[u8]) -> (u32, Vec<u8>) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("reqwestクライアントの構築に失敗");

    let result = match method {
        "GET" => client.get(url).send().await,
        "POST" => {
            client
                .post(url)
                .header("Content-Type", "application/json")
                .body(body.to_vec())
                .send()
                .await
        }
        other => {
            tracing::error!("未サポートのHTTPメソッド: {}", other);
            let msg = format!("Unsupported method: {}", other).into_bytes();
            return (400, msg);
        }
    };

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16() as u32;
            let body_bytes = resp.bytes().await.unwrap_or_default().to_vec();
            tracing::info!(
                "HTTP転送完了: status={}, body={} bytes",
                status,
                body_bytes.len()
            );
            (status, body_bytes)
        }
        Err(e) => {
            tracing::error!("HTTPリクエスト失敗: {}", e);
            let msg = format!("Proxy error: {}", e).into_bytes();
            (500, msg)
        }
    }
}

// ============================================================
// TCP接続ハンドラ（macOS / テスト用）
// ============================================================

/// 仕様書 §6.4
/// TCP経由の接続を処理する。非Linuxプラットフォームでのフォールバック。
/// vsockと同一のlength-prefixedプロトコルを使用。
#[cfg(any(not(target_os = "linux"), test))]
async fn handle_tcp_connection(mut stream: tokio::net::TcpStream) {
    let method = match read_string_async(&mut stream).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("メソッド読み取りエラー: {}", e);
            return;
        }
    };
    let url = match read_string_async(&mut stream).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("URL読み取りエラー: {}", e);
            return;
        }
    };
    let body = match read_bytes_async(&mut stream).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("ボディ読み取りエラー: {}", e);
            return;
        }
    };

    tracing::info!("{} {} (body: {} bytes)", method, url, body.len());

    let (status, resp_body) = forward_http(&method, &url, &body).await;

    if let Err(e) = write_response_async(&mut stream, status, &resp_body).await {
        tracing::error!("レスポンス書き込みエラー: {}", e);
    }
}

// ============================================================
// Linux vsock ハンドラ
// ============================================================

/// ストリームから4バイトビッグエンディアンのu32を同期的に読み取る。
#[cfg(target_os = "linux")]
fn read_u32_sync(r: &mut impl std::io::Read) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

/// length-prefixed文字列を同期的に読み取る。
#[cfg(target_os = "linux")]
fn read_string_sync(r: &mut impl std::io::Read) -> std::io::Result<String> {
    let len = read_u32_sync(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// length-prefixedバイト列を同期的に読み取る。
#[cfg(target_os = "linux")]
fn read_bytes_sync(r: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let len = read_u32_sync(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// プロキシレスポンスを同期的に書き込む: [4B: status][4B: body_len][body]
#[cfg(target_os = "linux")]
fn write_response_sync(
    w: &mut impl std::io::Write,
    status: u32,
    body: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    w.write_all(&status.to_be_bytes())?;
    w.write_all(&(body.len() as u32).to_be_bytes())?;
    w.write_all(body)?;
    w.flush()?;
    Ok(())
}

/// 仕様書 §6.4
/// vsock接続を処理する（Linux専用）。
/// ブロッキングI/Oは `spawn_blocking` でラップし、HTTP転送は非同期で行う。
#[cfg(target_os = "linux")]
async fn handle_vsock_connection(stream: vsock::VsockStream) {
    // vsockストリームからリクエストを読み取り（ブロッキング）
    let result = tokio::task::spawn_blocking(move || {
        let mut s = stream;
        let method = read_string_sync(&mut s)?;
        let url = read_string_sync(&mut s)?;
        let body = read_bytes_sync(&mut s)?;
        Ok::<_, std::io::Error>((s, method, url, body))
    })
    .await;

    let (stream, method, url, body) = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::error!("リクエスト読み取りエラー: {}", e);
            return;
        }
        Err(e) => {
            tracing::error!("spawn_blockingエラー: {}", e);
            return;
        }
    };

    tracing::info!("{} {} (body: {} bytes)", method, url, body.len());

    // 非同期でHTTP転送
    let (status, resp_body) = forward_http(&method, &url, &body).await;

    // vsockストリームにレスポンスを書き戻し（ブロッキング）
    let result = tokio::task::spawn_blocking(move || {
        let mut s = stream;
        write_response_sync(&mut s, status, &resp_body)
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!("レスポンス書き込みエラー: {}", e),
        Err(e) => tracing::error!("spawn_blockingエラー: {}", e),
    }
}

// ============================================================
// エントリポイント
// ============================================================

/// Linux: vsockでリッスン
#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("vsock HTTPプロキシを port {} で起動します", VSOCK_PORT);

    let listener =
        vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, VSOCK_PORT)?;

    // vsock acceptはブロッキングなので専用スレッドで実行し、
    // 受理した接続をmpscチャネルでtokioランタイムに渡す
    let (tx, mut rx) = tokio::sync::mpsc::channel::<vsock::VsockStream>(32);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    if tx.blocking_send(s).is_err() {
                        tracing::info!("チャネルクローズ、acceptループ終了");
                        break;
                    }
                }
                Err(e) => tracing::error!("vsock acceptエラー: {}", e),
            }
        }
    });

    while let Some(stream) = rx.recv().await {
        tokio::spawn(handle_vsock_connection(stream));
    }

    Ok(())
}

/// 非Linux: TCPフォールバックでリッスン（テスト・開発用）
#[cfg(not(target_os = "linux"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!(
        "TCPフォールバック HTTPプロキシを {} で起動します",
        TCP_ADDR
    );

    let listener = tokio::net::TcpListener::bind(TCP_ADDR).await?;

    loop {
        let (stream, addr) = listener.accept().await?;
        tracing::info!("TCP接続受付: {}", addr);
        tokio::spawn(handle_tcp_connection(stream));
    }
}

// ============================================================
// テスト
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Bytes, routing::get, routing::post, Router};
    use tokio::io::AsyncWriteExt;

    /// テスト用モックHTTPサーバーを起動し、ポート番号を返す。
    async fn start_mock_server() -> u16 {
        let app = Router::new()
            .route("/test", get(|| async { "hello" }))
            .route("/echo", post(|body: Bytes| async move { body }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    }

    /// テスト用プロキシサーバーを起動し、ポート番号を返す。
    async fn start_proxy() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(handle_tcp_connection(stream));
            }
        });
        port
    }

    /// クライアント側: ワイヤプロトコルでリクエストを送信
    async fn write_request<W: tokio::io::AsyncWrite + Unpin>(
        w: &mut W,
        method: &str,
        url: &str,
        body: &[u8],
    ) {
        w.write_all(&(method.len() as u32).to_be_bytes())
            .await
            .unwrap();
        w.write_all(method.as_bytes()).await.unwrap();
        w.write_all(&(url.len() as u32).to_be_bytes())
            .await
            .unwrap();
        w.write_all(url.as_bytes()).await.unwrap();
        w.write_all(&(body.len() as u32).to_be_bytes())
            .await
            .unwrap();
        w.write_all(body).await.unwrap();
        w.flush().await.unwrap();
    }

    /// クライアント側: ワイヤプロトコルでレスポンスを読み取り
    async fn read_response<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> (u32, Vec<u8>) {
        let status = read_u32_async(r).await.unwrap();
        let body = read_bytes_async(r).await.unwrap();
        (status, body)
    }

    /// GETリクエストのラウンドトリップテスト
    #[tokio::test]
    async fn test_get_roundtrip() {
        let server_port = start_mock_server().await;
        let proxy_port = start_proxy().await;

        let mut stream =
            tokio::net::TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
                .await
                .unwrap();

        let url = format!("http://127.0.0.1:{}/test", server_port);
        write_request(&mut stream, "GET", &url, &[]).await;

        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 200);
        assert_eq!(String::from_utf8(body).unwrap(), "hello");
    }

    /// POSTリクエストのラウンドトリップテスト
    #[tokio::test]
    async fn test_post_roundtrip() {
        let server_port = start_mock_server().await;
        let proxy_port = start_proxy().await;

        let mut stream =
            tokio::net::TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
                .await
                .unwrap();

        let url = format!("http://127.0.0.1:{}/echo", server_port);
        let payload = b"{\"key\":\"value\"}";
        write_request(&mut stream, "POST", &url, payload).await;

        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 200);
        assert_eq!(body, payload);
    }

    /// 未サポートメソッドで400が返ることを確認
    #[tokio::test]
    async fn test_unsupported_method() {
        let proxy_port = start_proxy().await;

        let mut stream =
            tokio::net::TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
                .await
                .unwrap();

        write_request(&mut stream, "DELETE", "http://example.com", &[]).await;

        let (status, body) = read_response(&mut stream).await;
        assert_eq!(status, 400);
        assert!(String::from_utf8(body)
            .unwrap()
            .contains("Unsupported method"));
    }
}
