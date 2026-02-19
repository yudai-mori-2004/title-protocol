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
//! - CID=3 (親インスタンス)
//! - Port=8000

/// vsockプロキシの接続先CID（親インスタンス）
const PROXY_CID: u32 = 3;
/// vsockプロキシの接続先ポート
const PROXY_PORT: u32 = 8000;

/// vsock経由のHTTPレスポンス。
pub struct ProxyResponse {
    /// HTTPステータスコード
    pub status: u32,
    /// レスポンスボディ
    pub body: Vec<u8>,
}

/// vsock経由でHTTP GETリクエストを送信する。
pub async fn proxy_get(_url: &str) -> Result<ProxyResponse, std::io::Error> {
    // TODO: vsock接続 (CID=PROXY_CID, Port=PROXY_PORT)
    // TODO: method="GET", url, body=空 を送信
    // TODO: レスポンスを受信して返却
    let _ = (PROXY_CID, PROXY_PORT);
    todo!("vsock経由HTTP GETの実装")
}

/// vsock経由でHTTP POSTリクエストを送信する。
pub async fn proxy_post(_url: &str, _body: &[u8]) -> Result<ProxyResponse, std::io::Error> {
    // TODO: vsock接続 (CID=PROXY_CID, Port=PROXY_PORT)
    // TODO: method="POST", url, body を送信
    // TODO: レスポンスを受信して返却
    let _ = (PROXY_CID, PROXY_PORT);
    todo!("vsock経由HTTP POSTの実装")
}
