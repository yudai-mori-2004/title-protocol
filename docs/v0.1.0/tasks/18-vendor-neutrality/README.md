# タスク18: ベンダー名の除去 + SDK粒度の再設計

## 概要

仕様書の「ベンダーロックインしない」設計思想をコード・環境変数・SDK APIに徹底的に反映する。
具体的には3つの変更を実施:

1. **MINIO_* → S3_* リネーム**: S3互換ストレージの環境変数からMinIOベンダー名を除去
2. **ARWEAVE_GATEWAY 除去**: TEEコードで未使用の設定項目を削除
3. **SDK StorageProvider 除去**: オフチェーンストレージ選択をプロトコル層から分離

## 設計思想

Title ProtocolのGatewayは `TempStorage` trait で任意のストレージバックエンドを受け入れる。
`S3TempStorage` はその1つの実装であり、環境変数 `S3_*` はS3実装固有の設定。
TEE Runtime（`TeeRuntime` trait → `MockRuntime` / `NitroRuntime`）と同じパターン。

```
TeeRuntime trait   → MockRuntime / NitroRuntime    （TEE_RUNTIME=mock|nitro で選択）
TempStorage trait  → S3TempStorage                 （S3_* 環境変数で設定）
                   → （将来: GcsTempStorage, AzureBlobTempStorage, ...）
```

SDKはGateway APIの薄いラッパーに徹し、ストレージ選択はユーザーに委ねる:

```
BEFORE: client.register() が内部で StorageProvider.upload() を呼ぶ
AFTER:  verify() → ユーザーが自由にストレージ保存 → sign() → ユーザーがウォレット署名
```

## 仕様書セクション

- §6.3 Temporary Storage（ストレージ抽象化）
- §6.7 SDK（公開API粒度）

## 前提タスク

- タスク01〜17全完了

## 変更内容

### Phase 1: MINIO_* → S3_* リネーム

| 環境変数 (BEFORE) | 環境変数 (AFTER) |
|---|---|
| `MINIO_ENDPOINT` | `S3_ENDPOINT` |
| `MINIO_PUBLIC_ENDPOINT` | `S3_PUBLIC_ENDPOINT` |
| `MINIO_ACCESS_KEY` | `S3_ACCESS_KEY` |
| `MINIO_SECRET_KEY` | `S3_SECRET_KEY` |
| `MINIO_BUCKET` | `S3_BUCKET` |
| `MINIO_REGION` | `S3_REGION` |

変更ファイル:

| ファイル | 変更箇所 |
|---------|---------|
| `crates/gateway/src/storage.rs` | `from_env()` の6つの env var名 + ログメッセージ |
| `.env.example` | 6変数のリネーム + コメント修正 |
| `docker-compose.yml` | gateway環境変数 (4箇所) |
| `deploy/setup-ec2.sh` | REQUIRED_VARS配列 + S3バケット確認ロジック |
| `scripts/setup-local.sh` | 変数名 |
| `tests/e2e/src/helpers.ts` | `MINIO_URL` → `S3_URL`, `fixMinioUrl` → `fixStorageUrl` |
| `tests/e2e/src/e2e.test.ts` | コメント修正 |

変えないもの:
- `docker-compose.yml` の `minio` サービス名、`MINIO_ROOT_USER/PASSWORD` — MinIOコンテナ自体の設定
- `scripts/setup-local.sh` の MinIO CLI (`mc`) コマンド — MinIO固有の操作

### Phase 2: ARWEAVE_GATEWAY / ARLOCAL_URL 除去

TEEコードで `ARWEAVE_GATEWAY` は一切読み取っていない。
TEEは `signed_json_uri` をクライアントから受け取り、プロキシ経由で汎用HTTP GETするだけ。

| ファイル | 変更 |
|---------|------|
| `.env.example` | `ARWEAVE_GATEWAY` 行を削除 |
| `docker-compose.yml` | tee-mock 環境変数から削除 |
| `deploy/setup-ec2.sh` | TEE起動時の環境変数から削除 |
| `tests/e2e/src/helpers.ts` | `ARLOCAL_URL` 定数を削除 |

### Phase 3: SDK StorageProvider 除去

| ファイル | 変更 |
|---------|------|
| `sdk/ts/src/storage.ts` | **削除** |
| `sdk/ts/src/register.ts` | **削除** |
| `sdk/ts/src/index.ts` | `storage`, `register` の re-export を削除 |
| `sdk/ts/src/client.ts` | `TitleClientConfig` から `storage` フィールドを削除 |
| `sdk/ts/src/types.ts` | `RegisterResult`, `ContentResult`, `ExtensionResult` を削除 |
| `tests/e2e/src/helpers.ts` | `setupClient()` から `storage` 引数を削除、`TestStorage` を SDK 非依存化 |
| `tests/e2e/src/e2e.test.ts` | `setupClient(storage)` → `setupClient()` |

残したもの:
- `client.ts` の `upload()` / `getUploadUrl()` — Gateway `/upload-url` への薄いラッパー

## 完了条件

- [x] 全ファイルで `MINIO_` 環境変数が `S3_` にリネーム済み（MinIOコンテナ固有設定を除く）
- [x] `ARWEAVE_GATEWAY` / `ARLOCAL_URL` が全ファイルから除去
- [x] `sdk/ts/src/storage.ts` と `sdk/ts/src/register.ts` が削除
- [x] `TitleClientConfig` から `storage` フィールドが削除
- [x] `cargo check --workspace` 通過
- [x] `cd sdk/ts && npm run build` 通過
- [x] `cd tests/e2e && npx tsc --noEmit` 通過
- [x] EC2 上で `.env` を `S3_*` に更新し、Gateway/TEE 再起動後にヘルスチェック通過
- [x] ローカルからPixel画像を `core-c2pa,phash-v1` で登録し、Solana devnet上でConfirmed

## 検証実績（2026-02-22）

EC2（35.77.196.129）で再ビルド・再起動後、ローカルから `register-content.mjs` で登録成功:

```
Image: PXL_20251216_122821334.jpg (2.19 MB)
Processors: core-c2pa, phash-v1
Owner: wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna

TX1 (core-c2pa):
  https://explorer.solana.com/tx/5ZZcwS3DHrn48WEJQJUbDGK7cXixMKqDDhoCFBPusesUgVcSueahc43kyM24DmVAQVNmhikoudcB8uCCwSPSx2m?cluster=devnet

TX2 (phash-v1):
  https://explorer.solana.com/tx/EWoeuoCShbbcbfSnAa9ChAZziFbWYDD5Es7Y1vCGP8ppuLaGJYvu6KCUSbNN1PXyMxBYoBqvpu3L974dvefX7fZ?cluster=devnet
```
