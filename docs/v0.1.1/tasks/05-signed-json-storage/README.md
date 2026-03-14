# Task 05: signed_json保存代行 — Gateway sign-and-mint拡張

## 目的

`/sign-and-mint` エンドポイントを拡張し、クライアントから `signed_json` 本体を直接受け取り、Gatewayが保存を代行する機能を追加する。これにより、クライアント側で別途ストレージを用意する必要がなくなり、SDK から verify → sign-and-mint の2ステップだけで登録が完結する。

## 背景

### 問題: storeSignedJson コールバックの必要性

現行の `delegateMint` フローでは、クライアントが `/verify` で受け取った signed_json を自前で保存し、URI を `/sign-and-mint` に渡す必要がある。これにはクライアント側にストレージ（Arweave, R2等）が必要で、完全なクライアント完結フローの障壁になっていた。

### 設計方針

- **プロトコル層は変更しない**: `signed_json_uri` を受け取る従来のフローが正式仕様
- **保存代行はノード運営者のオプション機能**: `/health` の `capabilities` で公開
- **後方互換**: `signed_json_uri` 指定（従来）と `signed_json` 本体指定（新規）の両方をサポート
- **保存先の永続性は実装依存**: S3のライフサイクル設定やArweaveの永続保証など、ノード運営者が保証する

## 設計

### フロー（新）

```
Client (SDK)
  │
  ├── GET /health → { capabilities: { store_signed_json: true } }
  │
  ├── POST /sign-and-mint
  │   { requests: [{ signed_json: {...} }] }  ← 本体を直接渡す（新）
  │   { requests: [{ signed_json_uri: "..." }] }  ← URI指定（従来通り）
  │
  │   Gateway内部:
  │     signed_json本体 → S3にPUT → パブリックURL取得
  │     → SignRequest { signed_json_uri: URL } を構築
  │     → TEE /sign に中継（従来フローに合流）
  │     → co-sign → broadcast → tx_signatures
```

### リクエスト形式（後方互換）

```json
// 従来（引き続きサポート）
{ "requests": [{ "signed_json_uri": "https://..." }] }

// 新規（signed_json本体）
{ "requests": [{ "signed_json": { "protocol": "Title-v1", ... } }] }
```

- `signed_json_uri` も `signed_json` も未指定 → 400 BadRequest
- `signed_json` 指定時に `signed_json_storage` 未設定 → 400 BadRequest（エラーメッセージでURI指定を案内）

### capabilities ディスカバラビリティ

```json
// GET /health レスポンス
{
  "status": "ok",
  "capabilities": {
    "store_signed_json": true
  }
}
```

SDKは `/health` を確認し、`store_signed_json: true` なら本体を直接送信、`false` なら従来通り自前保存してURIを渡す。

### S3バケット構成（Terraform）

| バケット | 用途 | ライフサイクル | アクセス |
|---------|------|-------------|---------|
| `title-uploads-devnet` | 暗号化ペイロード一時保管 | 1日で自動削除 | private |
| `title-signed-json-devnet` | signed_json保存 | なし | public-read |

両バケットは全ノードで共有（インスタンスごとではない）。

## 変更ファイル

### 新規作成

| ファイル | 内容 |
|---------|------|
| `crates/gateway/src/endpoints/health.rs` | `/health` ハンドラ（capabilities付き） |

### 変更（Rust — Gateway）

| ファイル | 変更内容 |
|---------|---------|
| `crates/gateway/src/storage/mod.rs` | `SignedJsonStorage` トレイト追加 |
| `crates/gateway/src/storage/s3.rs` | `S3SignedJsonStorage` 実装、`init_bucket` を `pub(crate)` に変更 |
| `crates/gateway/src/config.rs` | `GatewayState` に `signed_json_storage` フィールド追加 |
| `crates/gateway/src/endpoints/sign_and_mint.rs` | `SignAndMintInput/Item` 型追加、signed_json本体→保存→URI変換ロジック |
| `crates/gateway/src/endpoints/mod.rs` | `health` モジュール追加、テスト用型エクスポート |
| `crates/gateway/src/main.rs` | `signed_json_storage` 初期化、`/health` ハンドラ配線、テスト4件追加 |

### 変更（Terraform）

| ファイル | 変更内容 |
|---------|---------|
| `deploy/aws/terraform/main.tf` | `title-signed-json-devnet` バケット追加（public-read、ライフサイクルなし）、IAMポリシー更新 |
| `deploy/aws/terraform/variables.tf` | `signed_json_s3_bucket_name` 変数追加 |
| `deploy/aws/terraform/outputs.tf` | 新バケットの出力追加 |

## 環境変数（新規）

| 変数 | デフォルト | 説明 |
|------|----------|------|
| `SIGNED_JSON_S3_BUCKET` | （未設定=機能無効） | signed_json保存先S3バケット名 |
| `SIGNED_JSON_S3_PUBLIC_URL` | 自動構築 | パブリックURLベース（省略時はリージョンとバケット名から構築） |

## テスト

### 新規テスト（4件）

- `test_sign_and_mint_signed_json_no_storage` — signed_json本体でstorage未設定時に400エラー
- `test_sign_and_mint_missing_both` — signed_json_uriもsigned_jsonも未指定時に400エラー
- 既存テスト2件（`test_sign_and_mint_no_rpc_url`, `test_sign_and_mint_no_keypair`）を新リクエスト型に更新

### 既存テスト互換

- `cargo test --workspace` — 171件全パス

## 完了条件

- [x] `/sign-and-mint` が `signed_json` 本体を受け取り保存→URI変換できる
- [x] `/sign-and-mint` が `signed_json_uri` 指定の従来フローも引き続きサポートする
- [x] `/health` が `capabilities.store_signed_json` を返す
- [x] `SignedJsonStorage` トレイトが保存先に中立（S3/Arweave等を抽象化）
- [x] `S3SignedJsonStorage` が `SIGNED_JSON_S3_BUCKET` 環境変数で有効化される
- [x] Terraformに `title-signed-json-devnet` バケット（public-read、ライフサイクルなし）が追加されている
- [x] IAMポリシーが新バケットへのアクセスを許可している
- [x] 全既存テスト + 新規テスト4件がパスする
- [ ] EC2にデプロイしてGatewayを再起動する
- [ ] SDK側で `storeSignedJson` コールバックをオプション化する

## 参照

- `crates/gateway/src/storage/mod.rs` — `SignedJsonStorage` トレイト
- `crates/gateway/src/storage/s3.rs` — S3実装
- `crates/gateway/src/endpoints/sign_and_mint.rs` — ハンドラ
- `crates/gateway/src/endpoints/health.rs` — capabilities
- `deploy/aws/terraform/main.tf` — S3バケット定義
