# ベンダー実装ガイド

新しいベンダーを追加する際の設計指針。
`vendor-local` と `vendor-aws` の実装経験から抽出した原則を記録する。

---

## 1. ベンダー実装の自立性

**各ベンダー実装は、他のベンダーの存在を知らない。**

`deploy/local/` の中にいる開発者は、AWS という選択肢が存在することを知る必要がない。
逆も同様。ベンダー実装のコード・ドキュメント・スクリプトは、
自分自身だけで完結して説明可能でなければならない。

「比較して説明した方がわかりやすい」という誘惑に注意すること。
「S3の代わり」「AWSが不要」という説明は、読者にまずAWSを理解することを要求してしまう。
local を使いたい人にとって、AWS は無関係な情報でしかない。

**ベンダー間の関係を語れるのは、ベンダーの上位にいるドキュメントだけ。**
QUICKSTART.md のベンダー比較表、CLAUDE.md のアーキテクチャ概要がその場所。
個々のベンダーディレクトリの中では、自分のことだけを語る。

### チェックリスト

新しいベンダーの実装が完了したら、以下を確認する:

- [ ] `deploy/<vendor>/` 内の全ファイルに他ベンダー名（aws, local, ...）が登場しない
- [ ] `crates/*/src/storage/<vendor>.rs` に他ベンダーへの参照がない
- [ ] README.md や doc comment が、自分自身だけで意味が通る

---

## 2. プロトコルのどこがベンダー中立で、どこがベンダー固有か

```
┌─────────────────────────────────────────────────────────────────┐
│                        プロトコル層                              │
│                   （ベンダー中立、常に共通）                      │
│                                                                 │
│  crates/types     — データ構造                                   │
│  crates/crypto    — 暗号プリミティブ（ECDH, AES-GCM, Ed25519）  │
│  crates/core      — C2PA検証 + 来歴グラフ構築                    │
│  crates/wasm-host — WASM実行エンジン                             │
│  wasm/*           — WASMモジュール                               │
│  sdk/ts           — TypeScript SDK                              │
│  indexer          — cNFTインデクサ                                │
│  programs/        — Solanaプログラム                              │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                        trait 境界                                │
│                （ベンダーが実装すべきインターフェース）             │
│                                                                 │
│  TeeRuntime       — TEEランタイム抽象（鍵生成、署名、attestation）│
│  TempStorage      — 一時ストレージ抽象（URL生成）                 │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                       ベンダー実装層                              │
│              （ベンダーごとに独立、互いを知らない）                │
│                                                                 │
│  vendor-local:                                                  │
│    deploy/local/temp-storage/  — HTTPファイルサーバー             │
│    crates/gateway/.../local.rs — LocalTempStorage                │
│    crates/tee/.../mock.rs      — MockRuntime                     │
│    deploy/local/               — スクリプト、docker-compose       │
│                                                                 │
│  vendor-aws:                                                    │
│    crates/gateway/.../s3.rs    — S3TempStorage                   │
│    crates/tee/.../nitro.rs     — NitroRuntime                    │
│    crates/crypto/.../nitro.rs  — Nitro Attestation               │
│    crates/proxy/               — vsock プロキシ                   │
│    deploy/aws/                 — Terraform, Dockerfile, スクリプト│
└─────────────────────────────────────────────────────────────────┘
```

### trait 境界が意味すること

`TeeRuntime` と `TempStorage` が、ベンダーが実装すべき全て。
新しいベンダーを追加するとは、この2つの trait を実装し、
`deploy/<vendor>/` にデプロイ手段を用意すること。

プロトコル層のコードに触れる必要はない。
触れたくなったら、それは trait 境界の設計が不足しているサイン。

---

## 3. ベンダー固有コードの配置ルール

| 種類 | 配置先 | 例 |
|------|--------|-----|
| TempStorage trait 実装 | `crates/gateway/src/storage/<vendor>.rs` | `s3.rs`, `local.rs` |
| TeeRuntime trait 実装 | `crates/tee/src/runtime/<vendor>.rs` | `nitro.rs`, `mock.rs` |
| Attestation 検証 | `crates/crypto/src/attestation/<vendor>.rs` | `nitro.rs` |
| ネットワーク中継 | `crates/proxy/` (必要な場合のみ) | vsock proxy |
| 独立サーバー | `deploy/<vendor>/` 内 | `temp-storage/` |
| デプロイスクリプト | `deploy/<vendor>/` | `setup.sh` |
| インフラ定義 | `deploy/<vendor>/terraform/` 等 | `main.tf` |
| Docker 定義 | `deploy/<vendor>/docker/` | `tee.Dockerfile` |
| Cargo feature flag | 各 crate の `Cargo.toml` | `vendor-aws`, `vendor-local` |

### feature flag のルール

- feature flag 名: `vendor-<name>`
- デフォルト: `vendor-aws`（本番向け）
- ベンダー固有の依存は `optional = true` でゲート
- `create_temp_storage()` 等のファクトリ関数は `#[cfg(feature = "...")]` で分岐
- ベンダーなしビルド（`--no-default-features`）は起動時エラーで通知

---

## 4. 新しいベンダーを追加する手順

1. **trait 実装を書く**
   - `crates/gateway/src/storage/<vendor>.rs` — `TempStorage` trait
   - `crates/tee/src/runtime/<vendor>.rs` — `TeeRuntime` trait（必要なら）

2. **feature flag を追加する**
   - 各 crate の `Cargo.toml` に `vendor-<name>` feature
   - `storage/mod.rs`, `runtime/mod.rs` に conditional exports
   - `main.rs` のファクトリ関数に `#[cfg]` 分岐追加

3. **deploy/<vendor>/ を作る**
   - セットアップスクリプト
   - Docker Compose（必要なもの）
   - README.md（自立した説明）

4. **QUICKSTART.md にセクション追加**
   - ベンダー比較表に1行追加
   - デプロイセクション追加

5. **自立性を検証する**
   - `deploy/<vendor>/` 内に他ベンダー名が登場しないこと
   - README.md が自分だけで意味が通ること

---

## 5. よくある間違い

| やりがち | なぜダメか | 代わりにこうする |
|---------|-----------|----------------|
| 「S3の代わりに...」 | S3を知らない人には意味不明 | 機能を直接説明する |
| 「AWS不要の...」 | AWSが前提であるかのように聞こえる | 自分が何であるかだけ述べる |
| 「Enclaveの代わりにMockRuntime」 | Enclaveを知らない人には不要な情報 | MockRuntimeの説明だけする |
| 比較表を deploy/<vendor>/README に置く | 他ベンダーの知識を要求する | 比較表は上位ドキュメント(QUICKSTART等)に |
| SKIPステップ（「Step 3: SKIP」） | 他ベンダーのステップ構造がリーク | 自分に必要なステップだけ番号を振る |
