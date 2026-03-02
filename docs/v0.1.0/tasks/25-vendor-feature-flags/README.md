# タスク25: Cargo vendor-aws feature flags

## 概要

全Rustクレートに `vendor-aws` feature flag を導入し、
ベンダー固有コードを条件コンパイルでゲートする。

これにより:
- `cargo build` → AWS Nitro対応のフル実装（フォーク版）
- `cargo build --no-default-features` → MockRuntime + trait定義のみ（プロトコルOSS版）

が実現する。ファイルの場所は変更せず、既存のtrait抽象化を活かす。

## 参照

- OSS品質監査レポート §4「プロトコル vs ベンダー実装の分離」
- OSS品質監査レポート §4.A「Cargo feature flags」

## 前提タスク

- タスク23（storage/ ディレクトリ分離が完了し `storage/s3.rs` が存在すること）
- タスク24（infra/ ディレクトリ分離が完了していること）

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `crates/tee/Cargo.toml` | 現状の `cfg(target_os = "linux")` 依存 |
| `crates/tee/src/runtime/mod.rs` | `pub mod nitro;` のゲート対象 |
| `crates/tee/src/main.rs` | `"nitro"` match arm のゲート対象 |
| `crates/crypto/Cargo.toml` | 現状の依存（feature追加対象） |
| `crates/crypto/src/attestation/mod.rs` | `pub mod nitro;` + `verify_attestation()` のゲート対象 |
| `crates/gateway/Cargo.toml` | `rust-s3` 依存のゲート対象 |
| `crates/gateway/src/storage/mod.rs` | `pub mod s3;` のゲート対象（タスク23後） |
| `crates/gateway/src/main.rs` | S3TempStorage 構築部分のゲート対象 |
| `crates/proxy/Cargo.toml` | `vsock` 依存のゲート対象 |
| `crates/proxy/src/main.rs` | vsock リスナー部分のゲート対象 |
| `Cargo.toml`（workspace） | workspace 全体の feature 連動 |

## 作業内容

### 1. crates/tee — NitroRuntime のゲート

```toml
# crates/tee/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = []

[target.'cfg(all(target_os = "linux", feature = "vendor-aws"))'.dependencies]
aws-nitro-enclaves-nsm-api = "0.4"
```

```rust
// crates/tee/src/runtime/mod.rs
pub mod mock;
#[cfg(feature = "vendor-aws")]
pub mod nitro;
```

```rust
// crates/tee/src/main.rs（ランタイム選択部分）
let runtime: Box<dyn runtime::TeeRuntime + Send + Sync> = match runtime_name.as_str() {
    "mock" => Box::new(runtime::mock::MockRuntime::new()),
    #[cfg(feature = "vendor-aws")]
    "nitro" => Box::new(runtime::nitro::NitroRuntime::new()),
    other => anyhow::bail!("未対応のTEEランタイム: {other}"),
};
```

### 2. crates/crypto — Nitro Attestation のゲート

```toml
# crates/crypto/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = []
```

```rust
// crates/crypto/src/attestation/mod.rs
#[cfg(feature = "vendor-aws")]
pub mod nitro;

pub fn verify_attestation(tee_type: &str, document: &[u8]) -> Result<AttestationResult, AttestationError> {
    match tee_type {
        #[cfg(feature = "vendor-aws")]
        "aws_nitro" => {
            let nitro_result = nitro::verify_nitro_attestation(document)?;
            Ok(nitro_result.into())
        }
        other => Err(AttestationError::UnsupportedTeeType(other.into())),
    }
}
```

`From<NitroAttestationResult> for AttestationResult` の impl も `#[cfg(feature = "vendor-aws")]` でゲート。

### 3. crates/gateway — S3TempStorage のゲート

```toml
# crates/gateway/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = ["rust-s3"]

[dependencies]
rust-s3 = { workspace = true, optional = true }
```

```rust
// crates/gateway/src/storage/mod.rs
#[cfg(feature = "vendor-aws")]
pub mod s3;
#[cfg(feature = "vendor-aws")]
pub use s3::S3TempStorage;
```

`main.rs` の `S3TempStorage::from_env()` 呼び出し部分も `#[cfg(feature = "vendor-aws")]` でゲートし、
feature なしの場合はコンパイルエラーではなく起動時エラー（`TempStorage` の実装がない旨）にする。

### 4. crates/proxy — vsock のゲート

```toml
# crates/proxy/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = []

[target.'cfg(all(target_os = "linux", feature = "vendor-aws"))'.dependencies]
vsock = "0.4"
```

現状の `#[cfg(target_os = "linux")]` を `#[cfg(all(target_os = "linux", feature = "vendor-aws"))]` に変更。
TCP fallback はベンダー非依存なのでそのまま残す。

### 5. Workspace の feature 連動（任意）

workspace ルートの `Cargo.toml` に feature 定義を追加し、
`cargo build --features vendor-aws` でワークスペース全体を一括有効化できるようにする。

### 6. ビルド検証

```bash
# フォーク版（デフォルト = vendor-aws有効）
cargo check --workspace
cargo test --workspace

# プロトコルOSS版（vendor-aws無効）
cargo check --workspace --no-default-features
```

`--no-default-features` でコンパイルが通ることを確認。
テストは MockRuntime 依存のもののみ実行される。

## 対象ファイル一覧

| # | ファイル | 変更 |
|---|---------|------|
| 1 | `crates/tee/Cargo.toml` | `[features]` 追加、依存の条件変更 |
| 2 | `crates/tee/src/runtime/mod.rs` | `#[cfg(feature = "vendor-aws")]` 追加 |
| 3 | `crates/tee/src/main.rs` | match arm にcfg追加 |
| 4 | `crates/crypto/Cargo.toml` | `[features]` 追加 |
| 5 | `crates/crypto/src/attestation/mod.rs` | `#[cfg]` 追加（mod nitro, match arm, From impl） |
| 6 | `crates/gateway/Cargo.toml` | `[features]` 追加、`rust-s3` を optional に |
| 7 | `crates/gateway/src/storage/mod.rs` | `#[cfg]` 追加 |
| 8 | `crates/gateway/src/main.rs` | S3TempStorage 構築部分にcfg追加 |
| 9 | `crates/proxy/Cargo.toml` | `[features]` 追加 |
| 10 | `crates/proxy/src/main.rs` | `cfg(target_os)` → `cfg(all(...))` に拡張 |

## 完了条件

- [ ] 4クレート全てに `[features] default = ["vendor-aws"]` が定義されている
- [ ] `cargo check --workspace` 通過（デフォルト = vendor-aws有効）
- [ ] `cargo test --workspace` 通過（デフォルト）
- [ ] `cargo check --workspace --no-default-features` 通過（プロトコルOSS版）
- [ ] `--no-default-features` 時に nitro.rs, s3.rs, vsock が一切コンパイルされない
- [ ] `--no-default-features` 時に MockRuntime でTEEが起動可能
