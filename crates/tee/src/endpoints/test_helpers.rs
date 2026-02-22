//! # エンドポイントテスト用共通ヘルパー
//!
//! verify, signテストで共有するモックサーバー群。

/// テスト用モックHTTPサーバーを起動し、指定パスで指定データを返す。
pub async fn start_mock_storage(path: &str, data: Vec<u8>) -> u16 {
    use axum::routing::get;

    let app = axum::Router::new().route(
        path,
        get(move || {
            let d = data.clone();
            async move { d }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    port
}

/// テスト用インラインプロキシを起動する。
/// proxy crateのTCPフォールバックと同等のlength-prefixedプロトコルでHTTPリクエストを転送する。
pub async fn start_inline_proxy() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf4 = [0u8; 4];

                // Read method
                stream.read_exact(&mut buf4).await.unwrap();
                let method_len = u32::from_be_bytes(buf4) as usize;
                let mut method_buf = vec![0u8; method_len];
                stream.read_exact(&mut method_buf).await.unwrap();
                let method = String::from_utf8(method_buf).unwrap();

                // Read url
                stream.read_exact(&mut buf4).await.unwrap();
                let url_len = u32::from_be_bytes(buf4) as usize;
                let mut url_buf = vec![0u8; url_len];
                stream.read_exact(&mut url_buf).await.unwrap();
                let url = String::from_utf8(url_buf).unwrap();

                // Read body
                stream.read_exact(&mut buf4).await.unwrap();
                let body_len = u32::from_be_bytes(buf4) as usize;
                let mut body = vec![0u8; body_len];
                if body_len > 0 {
                    stream.read_exact(&mut body).await.unwrap();
                }

                // Forward via reqwest
                let client = reqwest::Client::new();
                let result = match method.as_str() {
                    "GET" => client.get(&url).send().await,
                    "POST" => client.post(&url).body(body).send().await,
                    _ => {
                        stream.write_all(&400u32.to_be_bytes()).await.unwrap();
                        let msg = b"Unsupported method";
                        stream
                            .write_all(&(msg.len() as u32).to_be_bytes())
                            .await
                            .unwrap();
                        stream.write_all(msg).await.unwrap();
                        return;
                    }
                };

                match result {
                    Ok(resp) => {
                        let status = resp.status().as_u16() as u32;
                        let resp_body = resp.bytes().await.unwrap_or_default();
                        stream.write_all(&status.to_be_bytes()).await.unwrap();
                        stream
                            .write_all(&(resp_body.len() as u32).to_be_bytes())
                            .await
                            .unwrap();
                        stream.write_all(&resp_body).await.unwrap();
                    }
                    Err(_) => {
                        stream.write_all(&500u32.to_be_bytes()).await.unwrap();
                        let msg = b"Proxy error";
                        stream
                            .write_all(&(msg.len() as u32).to_be_bytes())
                            .await
                            .unwrap();
                        stream.write_all(msg).await.unwrap();
                    }
                }
            });
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    port
}
