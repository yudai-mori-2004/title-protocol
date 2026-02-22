# タスク22: TEEクレート構造基盤整備

## 概要

TEEクレートにGatewayクレートと同水準の構造基盤を導入する。
具体的には、統一エラー型 `TeeError`、専用の状態構造体ファイル `config.rs`、
エンドポイントの barrel export を追加する。

これはタスク23〜24の前提となる基盤作業であり、先に行うことで
後続のファイル分割・再編成時の手戻りを防ぐ。

## 参照

- OSS品質監査レポート §1.1「TEEの状態オブジェクト名」
- OSS品質監査レポート §1.2「エラー型の非対称性」

## 前提タスク

- タスク01〜20全完了

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `crates/gateway/src/error.rs` | `GatewayError` の模範パターン（40行） |
| `crates/gateway/src/config.rs` | `GatewayState` の模範パターン（41行） |
| `crates/gateway/src/endpoints/mod.rs` | barrel export の模範パターン |
| `crates/tee/src/main.rs` | `AppState` 定義（L37-65）+ ランタイム初期化 |
| `crates/tee/src/endpoints/verify.rs` | `(StatusCode, String)` エラー返却の現状確認 |
| `crates/tee/src/endpoints/sign.rs` | 同上 |
| `crates/tee/src/endpoints/create_tree.rs` | 同上 |
| `crates/tee/src/endpoints/mod.rs` | barrel export がない現状確認 |

## 作業内容

### 1. TeeError enum の作成

`crates/tee/src/error.rs` を新規作成。`GatewayError` と同じパターンで:

```rust
#[derive(Debug, thiserror::Error)]
pub enum TeeError {
    #[error("不正なリクエスト: {0}")]
    BadRequest(String),
    #[error("暗号処理に失敗: {0}")]
    Crypto(String),
    #[error("C2PA検証に失敗: {0}")]
    Verification(String),
    #[error("WASM実行に失敗: {0}")]
    Wasm(String),
    #[error("Solanaトランザクション構築に失敗: {0}")]
    Solana(String),
    #[error("外部通信に失敗: {0}")]
    Proxy(String),
    #[error("内部エラー: {0}")]
    Internal(String),
    #[error("サーバーが{0}状態です")]
    InvalidState(String),
    #[error("Gateway認証に失敗: {0}")]
    Unauthorized(String),
}

impl axum::response::IntoResponse for TeeError { ... }
```

各バリアントに適切な `StatusCode` をマッピングする。

### 2. 全エンドポイントの戻り値を TeeError に統一

`verify.rs`, `sign.rs`, `create_tree.rs` の各ハンドラで:

**Before:**
```rust
async fn handle_verify(...) -> Result<Json<...>, (StatusCode, String)> {
    // ...
    Err((StatusCode::BAD_REQUEST, "error message".to_string()))
}
```

**After:**
```rust
async fn handle_verify(...) -> Result<Json<...>, TeeError> {
    // ...
    Err(TeeError::BadRequest("error message".into()))
}
```

### 3. AppState → TeeAppState + config.rs 移動

`crates/tee/src/config.rs` を新規作成し、`main.rs` から `AppState`（L37-65）と
`TeeState` enum（L28-34）を移動する。

- 構造体名を `AppState` → `TeeAppState` にリネーム
- `main.rs` は `use crate::config::{TeeAppState, TeeState};` で参照
- 全エンドポイントの `State<Arc<AppState>>` → `State<Arc<TeeAppState>>` に更新

### 4. endpoints/mod.rs に barrel export 追加

Gateway の `endpoints/mod.rs` と同じパターン:

```rust
pub mod create_tree;
pub mod sign;
pub mod verify;

pub use create_tree::handle_create_tree;
pub use sign::handle_sign;
pub use verify::handle_verify;
```

`main.rs` のルーター定義が `endpoints::handle_verify` に簡略化される。

## 対象ファイル一覧

| # | ファイル | 変更 |
|---|---------|------|
| 1 | `crates/tee/src/error.rs` | **新規** — TeeError enum |
| 2 | `crates/tee/src/config.rs` | **新規** — TeeAppState + TeeState |
| 3 | `crates/tee/src/main.rs` | AppState/TeeState を config.rs に移動、import更新 |
| 4 | `crates/tee/src/endpoints/mod.rs` | barrel export 追加 |
| 5 | `crates/tee/src/endpoints/verify.rs` | 戻り値を TeeError に変更 |
| 6 | `crates/tee/src/endpoints/sign.rs` | 戻り値を TeeError に変更 |
| 7 | `crates/tee/src/endpoints/create_tree.rs` | 戻り値を TeeError に変更 |

## 完了条件

- [ ] `crates/tee/src/error.rs` に `TeeError` enum が存在し、`IntoResponse` を実装
- [ ] `crates/tee/src/config.rs` に `TeeAppState` + `TeeState` が存在
- [ ] `main.rs` に `AppState` / `TeeState` の直接定義がない
- [ ] 全エンドポイントの戻り値が `Result<Json<T>, TeeError>`
- [ ] `endpoints/mod.rs` に `pub use` による barrel export がある
- [ ] `cargo check --workspace` 通過
- [ ] `cargo test --workspace` 通過
