# タスク14: hmac_content ホスト関数

## 仕様書

§7.1 — `hmac_content(algorithm, key, offset, length)` → MACバイト列

## 背景

WASMモジュール向けホスト関数として `hash_content` は実装済み。
HMAC版である `hmac_content` を同パターンで追加する。

## 読むべきファイル

1. `crates/wasm-host/src/lib.rs` — `hash_content` の実装パターン
2. `crates/wasm-host/Cargo.toml` — 依存クレート（`hmac` は workspace に定義済み）

## 要件

- `hmac_content(algorithm: u32, key_ptr: u32, key_len: u32, offset: u32, length: u32, out_ptr: u32) -> u32`
- algorithm: 0=HMAC-SHA256, 1=HMAC-SHA384, 2=HMAC-SHA512（`hash_content` と同一マッピング）
- key はWASMリニアメモリからの読み取り
- content は TEEホストメモリ（`state.content[offset..offset+length]`）
- 戻り値: 書き込んだバイト数（失敗時は0）
- テスト追加

## 完了条件

- `cargo test -p title-wasm-host` で hmac 関連テストが通る
- `cargo check --workspace` 警告ゼロ
