//! # vsock経由HTTPクライアント
//!
//! 仕様書 §6.4
//!
//! TEEはネットワークアクセスを持たないため、全ての外部HTTP通信は
//! vsockプロキシ経由で行う。prototypeのenclave-c2paで動作実証済みの
//! length-prefixedプロトコルを踏襲する。
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
//! ## 接続先
//! - Linux: vsock CID=3 (親インスタンス), Port=8000
//! - macOS: TCP 127.0.0.1:8000（テスト用フォールバック）

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// vsock経由のHTTPレスポンス。
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
/// それ以外の場合はTCPフォールバック用のアドレス（例: "127.0.0.1:8000"）として扱い、
/// length-prefixedプロトコルでプロキシに接続する。
/// Linux本番環境ではvsock接続を使用する（TODO: タスク11で実装）。
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

    // TODO: #[cfg(target_os = "linux")] vsock::VsockStream::connect_with_cid_port(3, 8000)
    // 現在はTCPフォールバックのみ実装
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

/// vsock経由でHTTP GETリクエストを送信する。
/// 仕様書 §6.4
pub async fn proxy_get(proxy_addr: &str, url: &str) -> Result<ProxyResponse, std::io::Error> {
    proxy_request(proxy_addr, "GET", url, &[]).await
}

/// vsock経由でHTTP POSTリクエストを送信する。
/// 仕様書 §6.4
pub async fn proxy_post(
    proxy_addr: &str,
    url: &str,
    body: &[u8],
) -> Result<ProxyResponse, std::io::Error> {
    proxy_request(proxy_addr, "POST", url, body).await
}
