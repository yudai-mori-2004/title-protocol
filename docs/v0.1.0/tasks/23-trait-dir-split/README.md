# タスク23: Trait実装のディレクトリ分離

## 概要

`TeeRuntime`（`runtime/mod.rs` + `mock.rs` + `nitro.rs`）の模範的なパターンに倣い、
`TempStorage` と `WasmLoader` のtrait定義と実装を個別ファイルに分離する。

現状では trait + 全実装が1ファイルに混在しており、新しいバックエンド
（GCS, R2, IPFS等）を追加する際にスケールしない。

## 参照

- OSS品質監査レポート §2.1「Trait実装のファイル分離」

## 前提タスク

- タスク22（TeeError + config.rs が存在すること）

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `crates/tee/src/runtime/mod.rs` | 模範パターン（traitのみ、62行） |
| `crates/tee/src/runtime/mock.rs` | 模範パターン（実装のみ） |
| `crates/tee/src/runtime/nitro.rs` | 模範パターン（実装のみ） |
| `crates/gateway/src/storage.rs` | 分離対象（trait + S3実装、162行） |
| `crates/tee/src/wasm_loader.rs` | 分離対象（trait + FileLoader + HttpLoader、124行） |
| `crates/gateway/src/main.rs` | storage import パスの更新先 |
| `crates/tee/src/main.rs` | wasm_loader import パスの更新先 |

## 作業内容

### 1. storage.rs → storage/ ディレクトリ分離

**Before:**
```
crates/gateway/src/
└── storage.rs    (trait + S3TempStorage、162行)
```

**After:**
```
crates/gateway/src/
└── storage/
    ├── mod.rs    (TempStorage trait + PresignedUrls のみ)
    └── s3.rs     (S3TempStorage 実装)
```

- `mod.rs` にはtrait定義 `TempStorage` + `PresignedUrls` 構造体のみ配置
- `s3.rs` に `S3TempStorage` 構造体、`new()`, `from_env()`, `init_bucket()`, `impl TempStorage for S3TempStorage` を配置
- `s3.rs` は `use super::{TempStorage, PresignedUrls};` で trait を参照
- `crates/gateway/src/main.rs` の `storage::S3TempStorage` → `storage::s3::S3TempStorage` に更新
  （ただし `mod.rs` で `pub use s3::S3TempStorage;` を re-export すれば変更不要）

### 2. wasm_loader.rs → wasm_loader/ ディレクトリ分離

**Before:**
```
crates/tee/src/
└── wasm_loader.rs    (trait + FileLoader + HttpLoader、124行)
```

**After:**
```
crates/tee/src/
└── wasm_loader/
    ├── mod.rs     (WasmLoader trait のみ)
    ├── file.rs    (FileLoader 実装)
    └── http.rs    (HttpLoader 実装)
```

- `mod.rs` には `WasmLoader` trait のみ配置
- `file.rs` に `FileLoader` 実装
- `http.rs` に `HttpLoader` 実装
- `crates/tee/src/main.rs` の import パスを更新
  （`mod.rs` で `pub use file::FileLoader; pub use http::HttpLoader;` を re-export すれば変更最小）

### 3. import パスの更新

各 `mod.rs` で主要型を re-export し、外部からの参照パスが変わらないようにする。

```rust
// crates/gateway/src/storage/mod.rs
pub mod s3;
pub use s3::S3TempStorage;

// crates/tee/src/wasm_loader/mod.rs
pub mod file;
pub mod http;
pub use file::FileLoader;
pub use http::HttpLoader;
```

## 対象ファイル一覧

| # | ファイル | 変更 |
|---|---------|------|
| 1 | `crates/gateway/src/storage.rs` | **削除** |
| 2 | `crates/gateway/src/storage/mod.rs` | **新規** — TempStorage trait + PresignedUrls |
| 3 | `crates/gateway/src/storage/s3.rs` | **新規** — S3TempStorage |
| 4 | `crates/tee/src/wasm_loader.rs` | **削除** |
| 5 | `crates/tee/src/wasm_loader/mod.rs` | **新規** — WasmLoader trait |
| 6 | `crates/tee/src/wasm_loader/file.rs` | **新規** — FileLoader |
| 7 | `crates/tee/src/wasm_loader/http.rs` | **新規** — HttpLoader |
| 8 | `crates/tee/src/main.rs` | wasm_loader の import パス更新（re-exportで最小化） |
| 9 | `crates/gateway/src/main.rs` | storage の import パス更新（re-exportで最小化） |

## 完了条件

- [ ] `crates/gateway/src/storage/mod.rs` に `TempStorage` trait のみ定義
- [ ] `crates/gateway/src/storage/s3.rs` に `S3TempStorage` 実装
- [ ] `crates/tee/src/wasm_loader/mod.rs` に `WasmLoader` trait のみ定義
- [ ] `crates/tee/src/wasm_loader/file.rs` に `FileLoader` 実装
- [ ] `crates/tee/src/wasm_loader/http.rs` に `HttpLoader` 実装
- [ ] 旧ファイル（`storage.rs`, `wasm_loader.rs`）が存在しない
- [ ] `cargo check --workspace` 通過
- [ ] `cargo test --workspace` 通過
