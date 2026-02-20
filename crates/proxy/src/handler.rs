//! # HTTP転送ハンドラ
//!
//! 仕様書 §6.4
//!
//! TEEから受け取ったHTTPリクエストを外部に転送し、レスポンスを返す。

use crate::protocol;

/// TEEから受け取ったHTTPリクエストを外部に転送し、レスポンスを返す。
/// 仕様書 §6.4
///
/// prototypeと同一のHTTPメソッドサポート（GET, POST）。
/// 未サポートのメソッドはステータス400を返す。
pub async fn forward_http(method: &str, url: &str, body: &[u8]) -> (u32, Vec<u8>) {
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

/// TCP経由の接続を処理する（非Linux / テスト用フォールバック）。
/// 仕様書 §6.4
///
/// vsockと同一のlength-prefixedプロトコルを使用。
#[cfg(any(not(target_os = "linux"), test))]
pub async fn handle_tcp_connection(mut stream: tokio::net::TcpStream) {
    let method = match protocol::read_string_async(&mut stream).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("メソッド読み取りエラー: {}", e);
            return;
        }
    };
    let url = match protocol::read_string_async(&mut stream).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("URL読み取りエラー: {}", e);
            return;
        }
    };
    let body = match protocol::read_bytes_async(&mut stream).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("ボディ読み取りエラー: {}", e);
            return;
        }
    };

    tracing::info!("{} {} (body: {} bytes)", method, url, body.len());

    let (status, resp_body) = forward_http(&method, &url, &body).await;

    if let Err(e) = protocol::write_response_async(&mut stream, status, &resp_body).await {
        tracing::error!("レスポンス書き込みエラー: {}", e);
    }
}

/// vsock接続を処理する（Linux専用）。
/// 仕様書 §6.4
///
/// ブロッキングI/Oは `spawn_blocking` でラップし、HTTP転送は非同期で行う。
#[cfg(target_os = "linux")]
pub async fn handle_vsock_connection(stream: vsock::VsockStream) {
    // vsockストリームからリクエストを読み取り（ブロッキング）
    let result = tokio::task::spawn_blocking(move || {
        let mut s = stream;
        let method = protocol::read_string_sync(&mut s)?;
        let url = protocol::read_string_sync(&mut s)?;
        let body = protocol::read_bytes_sync(&mut s)?;
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
        protocol::write_response_sync(&mut s, status, &resp_body)
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!("レスポンス書き込みエラー: {}", e),
        Err(e) => tracing::error!("spawn_blockingエラー: {}", e),
    }
}
