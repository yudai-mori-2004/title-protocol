//! # Title Protocol vsock HTTPプロキシ
//!
//! 仕様書 §6.4
//!
//! TEEにはネットワークアクセスがないため、全ての外部HTTP通信は
//! このプロキシを経由する。`prototype/enclave-c2pa/proxy/` をベースに
//! tokio非同期化した実装。
//!
//! ## プラットフォーム
//! - Linux: vsock port 8000 でリッスン
//! - macOS/Windows: TCP `127.0.0.1:8000` でリッスン（テスト用フォールバック）

mod handler;
mod protocol;

/// vsockリッスンポート
#[cfg(target_os = "linux")]
const VSOCK_PORT: u32 = 8000;

/// TCPフォールバックアドレス（macOS/テスト用）
#[cfg(not(target_os = "linux"))]
const TCP_ADDR: &str = "127.0.0.1:8000";

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
        tokio::spawn(handler::handle_vsock_connection(stream));
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
        tokio::spawn(handler::handle_tcp_connection(stream));
    }
}

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
                tokio::spawn(handler::handle_tcp_connection(stream));
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
        let status = protocol::read_u32_async(r).await.unwrap();
        let body = protocol::read_bytes_async(r).await.unwrap();
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
