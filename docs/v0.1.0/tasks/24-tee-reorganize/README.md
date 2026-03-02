# タスク24: TEEエンドポイント分割 + モジュール再編成

## 概要

TEEクレートの2つの構造問題を解決する:

1. **verify.rs の肥大化**（1,150行）— handler/core処理/extension処理/テストが1ファイルに混在
2. **フラットファイルの散在** — `gateway_auth.rs`, `proxy_client.rs`, `security.rs`, `solana_tx.rs` が
   ルートに無秩序に配置されている

Gatewayクレートの整理された構造を参考に、TEEクレートの可読性を向上させる。

## 参照

- OSS品質監査レポート §2.2「verify.rs の肥大化」
- OSS品質監査レポート §3.1「TEEクレートのフラット構造」

## 前提タスク

- タスク22（TeeError + TeeAppState + barrel export が存在すること）

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `crates/tee/src/endpoints/verify.rs` | 分割対象（1,150行） |
| `crates/tee/src/endpoints/sign.rs` | 構造比較（584行、分割不要の参考） |
| `crates/tee/src/gateway_auth.rs` | 移動対象（203行） |
| `crates/tee/src/proxy_client.rs` | 移動対象（151行） |
| `crates/tee/src/security.rs` | 移動対象（583行） |
| `crates/tee/src/solana_tx.rs` | 移動対象（391行） |
| `crates/tee/src/main.rs` | mod 宣言の更新先 |

## 作業内容

### 1. verify.rs → verify/ ディレクトリ分割

verify.rs の構成を分析し、以下のように分割する:

**Before:**
```
crates/tee/src/endpoints/
├── verify.rs          (1,150行: handler + core + extension + utils + tests)
├── sign.rs
└── create_tree.rs
```

**After:**
```
crates/tee/src/endpoints/
├── verify/
│   ├── mod.rs         (pub use + 共通ユーティリティ)
│   ├── handler.rs     (handle_verify メインハンドラ)
│   ├── core.rs        (process_core — C2PA検証フロー)
│   ├── extension.rs   (process_extension — WASM実行フロー)
│   └── tests.rs       (全テスト)
├── sign.rs
└── create_tree.rs
```

分割の指針:
- `handler.rs`: `handle_verify()` 関数 + リクエスト前処理（暗号化ペイロード復号等）
- `core.rs`: `process_core()` 関数 + core-c2pa 固有のロジック
- `extension.rs`: `process_extension()` 関数 + WASM実行ロジック
- `tests.rs`: `#[cfg(test)] mod tests { ... }` をそのまま移動
- `mod.rs`: MIME検出ユーティリティ等の共通関数 + `pub use handler::handle_verify;`

### 2. フラットファイルをサブモジュールに整理

**Before:**
```
crates/tee/src/
├── gateway_auth.rs     (認証)
├── proxy_client.rs     (外部通信)
├── security.rs         (リソース制限)
├── solana_tx.rs        (Bubblegumトランザクション)
└── ...
```

**After:**
```
crates/tee/src/
├── infra/
│   ├── mod.rs           (pub use)
│   ├── proxy_client.rs  (vsock/HTTP プロキシクライアント)
│   ├── security.rs      (DoS対策・リソース制限)
│   └── gateway_auth.rs  (Gateway認証検証)
├── blockchain/
│   ├── mod.rs           (pub use)
│   └── solana_tx.rs     (Bubblegum cNFTトランザクション構築)
└── ...
```

### 3. main.rs の mod 宣言・import 更新

```rust
// Before
mod gateway_auth;
mod proxy_client;
pub mod security;
mod solana_tx;

// After
mod infra;
mod blockchain;
```

各 `mod.rs` で必要な型を re-export し、エンドポイントからの参照パスを更新する。
例: `crate::proxy_client::ProxyHttpClient` → `crate::infra::proxy_client::ProxyHttpClient`
（`infra/mod.rs` で `pub use proxy_client::ProxyHttpClient;` すれば `crate::infra::ProxyHttpClient`）

## 対象ファイル一覧

| # | ファイル | 変更 |
|---|---------|------|
| 1 | `crates/tee/src/endpoints/verify.rs` | **削除**（ディレクトリに置換） |
| 2 | `crates/tee/src/endpoints/verify/mod.rs` | **新規** |
| 3 | `crates/tee/src/endpoints/verify/handler.rs` | **新規** |
| 4 | `crates/tee/src/endpoints/verify/core.rs` | **新規** |
| 5 | `crates/tee/src/endpoints/verify/extension.rs` | **新規** |
| 6 | `crates/tee/src/endpoints/verify/tests.rs` | **新規** |
| 7 | `crates/tee/src/gateway_auth.rs` | **移動** → `infra/gateway_auth.rs` |
| 8 | `crates/tee/src/proxy_client.rs` | **移動** → `infra/proxy_client.rs` |
| 9 | `crates/tee/src/security.rs` | **移動** → `infra/security.rs` |
| 10 | `crates/tee/src/solana_tx.rs` | **移動** → `blockchain/solana_tx.rs` |
| 11 | `crates/tee/src/infra/mod.rs` | **新規** |
| 12 | `crates/tee/src/blockchain/mod.rs` | **新規** |
| 13 | `crates/tee/src/main.rs` | mod 宣言 + import 更新 |
| 14 | `crates/tee/src/endpoints/mod.rs` | verify モジュールパス更新 |
| 15 | `crates/tee/src/endpoints/sign.rs` | import パス更新（infra::, blockchain::） |
| 16 | `crates/tee/src/endpoints/create_tree.rs` | import パス更新 |

## 完了条件

- [ ] `crates/tee/src/endpoints/verify/` ディレクトリに5ファイルが存在
- [ ] 旧 `verify.rs` が存在しない
- [ ] `crates/tee/src/infra/` に `proxy_client.rs`, `security.rs`, `gateway_auth.rs` が存在
- [ ] `crates/tee/src/blockchain/` に `solana_tx.rs` が存在
- [ ] ルートレベルに `gateway_auth.rs`, `proxy_client.rs`, `security.rs`, `solana_tx.rs` が存在しない
- [ ] `cargo check --workspace` 通過
- [ ] `cargo test --workspace` 通過
