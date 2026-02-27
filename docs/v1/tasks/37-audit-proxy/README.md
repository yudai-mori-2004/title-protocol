# Task 37: コード監査 — crates/proxy

## 対象
`crates/proxy/` — TEE HTTPプロキシ（length-prefixedバイナリプロトコル）

## ファイル
- `src/main.rs` — エントリポイント（vsock/TCP切替）、テスト3件
- `src/protocol.rs` — length-prefixed読み書き（async + sync）
- `src/handler.rs` — forward_http + 接続ハンドラ（TCP/vsock）

## 監査で発見された問題

### バグ
なし。コードはシンプルで正しい。

### コード品質
なし。全5依存クレートが実際に使用されている。

### テストギャップ
1. **HTTP転送失敗時のエラー伝播テストがない**:
   `forward_http`は接続失敗時に`(500, "Proxy error: ...")`を返すが、このパスが未検証。
   → テスト追加（+1）。

### 設計メモ（修正不要）
- 1リクエスト/1コネクション — proxy_client.rsと一貫性あり。
- reqwest::Client を毎回生成 — TEEプロキシの低トラフィック特性上、許容範囲。
- POST に Content-Type: application/json を固定 — TEEは現在GETのみ使用。POST対応は将来必要時に拡張。
- protocol読み取りにサイズ上限なし — 接続元はTEE(vsock)またはlocalhost(開発)のみで信頼済み。
- `#[cfg]` ガード（Linux/vendor-aws分岐）は正しく排他的。
- 全5依存クレートが適切に使用されている。

## 完了基準
- [x] HTTP転送失敗テスト追加（+1）
- [x] `cargo test -p title-proxy` パス（4テスト: 既存3 + 新規1）
- [x] `cargo check --workspace` パス

## 対処内容

### 1. HTTP転送失敗テスト追加（+1テスト）
- `test_forward_unreachable` — 到達不能なアドレス（127.0.0.1:1）へGETを転送し、ステータス500 + "Proxy error"メッセージが返ることを確認。
