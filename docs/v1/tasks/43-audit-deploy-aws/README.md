# Task 43: コード監査 — deploy/aws/

## 対象
`deploy/aws/` — AWS Nitro Enclaveデプロイ一式（Terraform, Docker, シェルスクリプト）

## ファイル
- `README.md` — デプロイ手順書
- `setup-ec2.sh` — メインデプロイスクリプト（8ステップ）
- `build-enclave.sh` — EIF単体ビルド
- `docker-compose.production.yml` — 本番Compose（Gateway + Indexer + PostgreSQL）
- `docker/tee.Dockerfile` — TEE Enclaveマルチステージビルド
- `docker/entrypoint.sh` — Enclave内エントリポイント（socat bridge）
- `terraform/main.tf` — EC2, S3, IAM, SecurityGroup
- `terraform/variables.tf` — Terraform変数
- `terraform/outputs.tf` — Terraform出力値
- `terraform/user-data.sh` — EC2初期セットアップ

## 監査で発見された問題

### バグ
1. **`setup-ec2.sh`: `S3_BUCKET`がREQUIRED_VARSに含まれていない**:
   L303で `S3_BUCKET` が未設定の場合 `title-uploads` にフォールバックするが、
   Terraformのデフォルトは `title-uploads-devnet`。ユーザーが`.env`で`S3_BUCKET`を
   設定し忘れると、Gateway が存在しないバケットにアクセスし失敗する。
   → REQUIRED_VARSに追加。

### コード品質
2. **`scripts/README.md`: Node.js "24+"は過剰**:
   スクリプトが使用する最新API（`AbortSignal.timeout`, global `fetch`）はNode 18+で利用可能。
   `user-data.sh`は`nodejs20`をインストールしており矛盾する。
   → "20+"に修正。

### 設計メモ（修正不要）
- `.gitignore`で`keys/`, `*.pem`, `*.tfstate`, `.terraform/`が正しく除外されている
- S3 CORS `allowed_origins: ["*"]`はpresigned URL直接アップロードに必要
- SG port 5000公開はDAS webhookに必要
- `tee.Dockerfile`の`.env`ベイクはNitro Enclave設計上必要（ホストFSアクセス不可）
- PostgreSQLポートは`127.0.0.1:5432`に正しくバインド（外部非公開）
- IAMはS3アクセスのみの最小権限
- `setup-ec2.sh`のMinIOパスはAWS以外のS3互換エンドポイント用で、本番では到達しない
- `entrypoint.sh`のsocat bridge設計（vsock↔TCP双方向）は正しい

## 完了基準
- [x] `setup-ec2.sh`: `S3_BUCKET`をREQUIRED_VARSに追加
- [x] `scripts/README.md`: Node.js要件を"20+"に修正

## 対処内容

### 1. S3_BUCKET を REQUIRED_VARS に追加
`setup-ec2.sh` L60: REQUIRED_VARSに`S3_BUCKET`を追加。
未設定時に`title-uploads`（Terraformデフォルトと不一致）にフォールバックする問題を解消。
設定漏れ時に即座にエラーメッセージを表示するようになった。

### 2. Node.js バージョン要件の修正
`scripts/README.md`: "Node.js 24+" → "Node.js 20+"。
スクリプトが使用するAPIは全てNode 18+で利用可能。`user-data.sh`のnodejs20インストールとも整合。
