// SPDX-License-Identifier: Apache-2.0

//! # TEE外部通信プロキシクライアント
//!
//! 仕様書 §6.4
//!
//! TEEはネットワーク隔離されているため、外部HTTP通信はプロキシ経由で行う。
//! length-prefixedプロトコルを使用する。
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
//! ## 接続モード
//! - 本番: PROXY_ADDR(TCP) → socat → vsock → ホスト側proxy
//! - 開発: PROXY_ADDR="direct" で直接HTTP

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// プロキシ経由のHTTPレスポンス。
#[derive(Debug)]
pub struct ProxyResponse {
    /// HTTPステータスコード
    pub status: u32,
    /// レスポンスボディ
    pub body: Vec<u8>,
}

/// Direct HTTPモード: プロキシプロトコルを経由せず直接HTTPリクエストを送信する。
/// Docker Compose環境（ローカル開発）ではTEEにネットワーク制限がないため、
/// PROXY_ADDR=direct で直接HTTP通信を行う。
async fn direct_http_request(
    method: &str,
    url: &str,
    body: &[u8],
) -> Result<ProxyResponse, std::io::Error> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let result = match method {
        "GET" => client.get(url).send().await,
        "POST" => client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_vec())
            .send()
            .await,
        other => {
            return Ok(ProxyResponse {
                status: 400,
                body: format!("Unsupported method: {other}").into_bytes(),
            });
        }
    };

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16() as u32;
            let resp_body = resp
                .bytes()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
                .to_vec();
            Ok(ProxyResponse {
                status,
                body: resp_body,
            })
        }
        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
    }
}

/// length-prefixedプロトコルでプロキシにHTTPリクエストを送信する。
/// 仕様書 §6.4
///
/// `proxy_addr` が `"direct"` の場合、プロキシプロトコルを経由せず
/// 直接HTTPリクエストを送信する（Docker Compose / ローカル開発用）。
///
/// それ以外の場合はTCPアドレス（例: "127.0.0.1:8000"）として扱い、
/// length-prefixedプロトコルでプロキシに接続する。
/// 本番環境ではTEE VM内のsocatがこのTCPポートをvsockにブリッジする。
async fn proxy_request(
    proxy_addr: &str,
    method: &str,
    url: &str,
    body: &[u8],
) -> Result<ProxyResponse, std::io::Error> {
    // Direct HTTPモード: プロキシを経由せず直接リクエスト
    if proxy_addr == "direct" {
        return direct_http_request(method, url, body).await;
    }

    // TEE VM内ではsocatがTCP→vsockをブリッジするため、常にTCP接続を使用する
    let mut stream = tokio::net::TcpStream::connect(proxy_addr).await?;

    // method
    let method_bytes = method.as_bytes();
    stream
        .write_all(&(method_bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(method_bytes).await?;

    // url
    let url_bytes = url.as_bytes();
    stream
        .write_all(&(url_bytes.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(url_bytes).await?;

    // body
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .await?;
    if !body.is_empty() {
        stream.write_all(body).await?;
    }
    stream.flush().await?;

    // response: status
    let mut buf4 = [0u8; 4];
    stream.read_exact(&mut buf4).await?;
    let status = u32::from_be_bytes(buf4);

    // response: body
    stream.read_exact(&mut buf4).await?;
    let body_len = u32::from_be_bytes(buf4) as usize;
    let mut resp_body = vec![0u8; body_len];
    if body_len > 0 {
        stream.read_exact(&mut resp_body).await?;
    }

    Ok(ProxyResponse {
        status,
        body: resp_body,
    })
}

/// プロキシ経由でHTTP GETリクエストを送信する。
/// 仕様書 §6.4
pub async fn proxy_get(proxy_addr: &str, url: &str) -> Result<ProxyResponse, std::io::Error> {
    proxy_request(proxy_addr, "GET", url, &[]).await
}

