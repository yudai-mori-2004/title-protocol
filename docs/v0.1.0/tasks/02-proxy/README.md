# タスク2: Proxy非同期化

## 読むべきファイル

1. `prototype/enclave-c2pa/proxy/src/main.rs` — 動作実証済みの同期版（**これを非同期化する**）
2. `crates/proxy/src/main.rs` — 現在のスタブ
3. `crates/proxy/Cargo.toml` — 依存関係

## 作業内容

`prototype/enclave-c2pa/proxy/src/main.rs` を参考に、`crates/proxy/src/main.rs` をtokio非同期版として実装する。

### 要件

- vsock port 8000 でリッスン
- プロトコルはprototypeと同一（length-prefixed: method, url, body）
  - TEE→Proxy: `[4B: method_len][method][4B: url_len][url][4B: body_len][body]`
  - Proxy→TEE: `[4B: status_code][4B: body_len][body]`
- `reqwest` の非同期クライアントを使用（`reqwest::blocking` は使わない）
- `tokio::spawn` で並行処理（prototypeの `std::thread::spawn` から改善）
- エラーハンドリングを `anyhow`/`thiserror` で適切に
- **Linux以外ではTCP `localhost:8000` でリッスン**（テスト用フォールバック）

### 非同期化のポイント

- `vsock::VsockStream` は `std::io::Read/Write` を実装する同期ストリーム。`tokio::io::AsyncRead/AsyncWrite` ではない
- `tokio::task::spawn_blocking` でブロッキングI/Oをラップするか、`tokio::net::TcpStream` 等を使ったアダプタを検討
- reqwestの非同期呼び出し部分は `.await` を使用

### TCP フォールバック（macOS/テスト用）

`#[cfg(not(target_os = "linux"))]` のmain関数で `tokio::net::TcpListener::bind("127.0.0.1:8000")` を使い、同一プロトコルでリッスンする。こちらは完全にasync/awaitで実装可能。

## 完了条件

- `cargo build -p title-proxy` がLinuxとmacOS両方で通る
- `#[cfg(test)]` で、TCPフォールバック経由のGET/POSTラウンドトリップテスト
  - テスト内でTCPリスナーを起動し、reqwestでモックHTTPサーバーに転送、レスポンスを検証
- `docs/COVERAGE.md` の該当箇所を更新
