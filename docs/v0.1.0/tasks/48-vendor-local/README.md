# タスク48: vendor-local ベンダー実装

## 概要

ローカル開発環境を `vendor-local` として実装する。
`deploy/local/` を新設し、QUICKSTART.md から直接ローカルでプロトコルスタック全体を起動できるようにする。

## 背景

QUICKSTART.md のゼロベーステスト（タスク47の再実施）で判明した障壁:
Phase 2（ノード起動）がAWS EC2前提で、ローカルで試す手段がなかった。

## 作業内容

### 1. TempStorage サーバー (`deploy/local/temp-storage/`)

素朴なHTTP PUT/GETファイルサーバー。独立プロセスとして port 3001 で動作。

- `deploy/local/temp-storage/Cargo.toml`
- `deploy/local/temp-storage/src/main.rs`
- workspace の `Cargo.toml` にメンバー追加

### 2. Gateway の vendor-local 実装

- `crates/gateway/src/storage/local.rs` — `LocalTempStorage` 実装
- `crates/gateway/Cargo.toml` — `vendor-local = []` feature 追加
- `crates/gateway/src/storage/mod.rs` — conditional exports 追加
- `crates/gateway/src/main.rs` — `create_temp_storage()` の 3パターン `#[cfg]` 分岐

### 3. deploy/local/ スクリプト・設定

- `deploy/local/setup.sh` — ビルド + 全プロセス起動 + ノード登録 + ヘルスチェック
- `deploy/local/teardown.sh` — 全プロセス停止
- `deploy/local/docker-compose.yml` — PostgreSQL のみ
- `deploy/local/README.md`

### 4. ドキュメント更新

- `QUICKSTART.md` — Phase 2 にローカルデプロイセクション追加
- `.env.example` — `LOCAL_STORAGE_*` 変数追加

## 設計判断

ベンダー実装の設計指針を `VENDOR-IMPLEMENTATION-GUIDE.md` に記録した。

## 完了条件

- [x] `cargo check --workspace` 通過（vendor-aws デフォルト）
- [x] `cargo test --workspace` 通過（151テスト全通過）
- [x] `cargo check -p title-gateway --no-default-features --features vendor-local` 通過
- [x] `deploy/local/` 内のファイルに他ベンダーへの参照がない
- [x] `crates/gateway/src/storage/local.rs` に他ベンダーへの参照がない
- [x] `deploy/aws/` に変更がない
