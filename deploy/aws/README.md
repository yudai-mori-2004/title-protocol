# AWS Nitro Enclave Node Deployment Guide

Title Protocol ノードを空の AWS アカウントからゼロベースで構築する手順書。

## アーキテクチャ

```
                    ┌── EC2 Instance (c5.xlarge) ─────────────────────┐
                    │                                                  │
Internet ─:3000──►  │  Docker Compose                                  │
                    │  ┌──────────┐  ┌──────────┐  ┌──────────┐      │
                    │  │ Gateway  │  │ Indexer   │  │ Postgres │      │
                    │  └────┬─────┘  └──────────┘  └──────────┘      │
                    │       │ :4000                                    │
                    │       ▼                                          │
                    │  socat (TCP:4000 ←► vsock)                      │
                    │       │                                          │
                    │  ┌────▼──────────────────────────┐              │
                    │  │  Nitro Enclave (EIF)           │              │
                    │  │  ┌─────────┐  ┌────────────┐  │              │
                    │  │  │ TEE     │  │ WASM       │  │              │
                    │  │  │ Server  │  │ Modules x4 │  │              │
                    │  │  └────┬────┘  └────────────┘  │              │
                    │  │       │ :8000                  │              │
                    │  │  socat (vsock ←► TCP:8000)     │              │
                    │  └───────┼────────────────────────┘              │
                    │          ▼                                        │
                    │  title-proxy (HTTP ←► 外部API)                   │
                    │                                                  │
                    │  S3 (Temp Storage) ◄── IAM User credentials     │
                    └──────────────────────────────────────────────────┘
```

- **Gateway**: HTTP API サーバー。クライアントからのリクエストを受け、TEE に中継
- **TEE**: Nitro Enclave 内で動作。C2PA 検証、WASM 実行、cNFT 署名を行う
- **Proxy**: vsock 経由で TEE の外部 HTTP 通信を中継（Solana RPC、Arweave 等）
- **Indexer**: cNFT の発行を監視し、PostgreSQL に記録
- **S3**: 暗号化されたコンテンツの一時保管（1 日で自動削除）

## 前提条件

- AWS アカウント（AdministratorAccess 権限のある IAM ユーザー推奨）
- AWS CLI 設定済み（`aws configure`）
- Terraform 1.5+
- Git

## Step 1: SSH キーペアの準備

```bash
mkdir -p deploy/aws/keys

# AWS にキーペアを作成
aws ec2 create-key-pair \
  --key-name title-protocol-devnet \
  --query 'KeyMaterial' \
  --output text > deploy/aws/keys/title-protocol-devnet.pem

chmod 400 deploy/aws/keys/title-protocol-devnet.pem
```

既存のキーペアを使う場合は `deploy/aws/keys/` に `.pem` ファイルを配置し、
`variables.tf` の `key_name` を合わせる。

## Step 2: Terraform で AWS リソースを作成

```bash
cd deploy/aws/terraform

terraform init
terraform plan     # 作成されるリソースを確認
terraform apply    # 実行（yes で確定）
```

### 作成されるリソース

| リソース | 用途 |
|---------|------|
| EC2 (c5.xlarge) | Nitro Enclave 対応インスタンス。Amazon Linux 2023 |
| S3 バケット | 暗号化コンテンツの一時保管。1 日ライフサイクルで自動削除 |
| IAM ユーザー + アクセスキー | Gateway の S3 認証用（永続キー） |
| Security Group | SSH:22, Gateway:3000, Indexer:5000 を開放 |

### Terraform 変数

| 変数 | デフォルト | 説明 |
|------|----------|------|
| `aws_region` | `ap-northeast-1` | AWS リージョン |
| `instance_type` | `c5.xlarge` | Nitro 対応必須。4 vCPU, 8 GB RAM |
| `key_name` | `title-protocol-devnet` | EC2 キーペア名 |
| `key_file` | `../keys/title-protocol-devnet.pem` | SSH 秘密鍵ファイルのパス |
| `allowed_ssh_cidrs` | `["0.0.0.0/0"]` | SSH 許可 CIDR ブロック |
| `project_name` | `title-protocol` | リソース命名用プロジェクト名 |
| `s3_bucket_name` | `title-uploads-devnet` | S3 バケット名（グローバルで一意） |
| `volume_size` | `50` | EBS ボリューム (GB) |
| `enclave_cpu_count` | `2` | Enclave 割当 vCPU（ホストから取得） |
| `enclave_memory_mib` | `1024` | Enclave 割当メモリ (MiB) |

## Step 3: .env の設定

Terraform の出力値を取得:

```bash
terraform output instance_public_ip
terraform output s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output s3_bucket_name
terraform output s3_bucket_endpoint
```

## Step 4: EC2 に SSH してデプロイ

```bash
# SSH 接続
ssh -i deploy/aws/keys/title-protocol-devnet.pem \
  ec2-user@$(terraform output -raw instance_public_ip)
```

EC2 上で:

```bash
# リポジトリをクローン
git clone <REPO_URL> ~/title-protocol
cd ~/title-protocol

# .env を作成
cp .env.example .env
# vim .env で Terraform output の値を設定:
#   SOLANA_RPC_URL=https://api.devnet.solana.com
#   S3_BUCKET=<terraform output s3_bucket_name>
#   S3_ENDPOINT=<terraform output s3_bucket_endpoint>
#   S3_REGION=ap-northeast-1
#   S3_ACCESS_KEY=<terraform output s3_access_key_id>
#   S3_SECRET_KEY=<terraform output -raw s3_secret_access_key>
#   DB_PASSWORD=<任意のパスワード>

# デプロイ実行（全自動）
./deploy/aws/setup-ec2.sh
```

### setup-ec2.sh が行うこと

| Step | 内容 |
|------|------|
| 0 | .env の読み込みと検証 |
| 1 | WASM モジュール 4 個のビルド |
| 1B | ホスト側バイナリのビルド（title-proxy） |
| 2 | TEE Docker イメージ → EIF (Enclave Image File) のビルド |
| 3 | Nitro Enclave の起動 + vsock ブリッジ設定 |
| 4 | Proxy の起動 |
| 5 | Docker Compose（Gateway + Indexer + PostgreSQL）の起動 |
| 6 | S3 アクセスの検証 |
| 7 | GlobalConfig 初期化 (`init-devnet.mjs`) |
| 8 | ヘルスチェック（Solana RPC, Gateway, TEE, Indexer） |

所要時間: 初回は Rust ビルドを含むため 15-30 分程度。

## Step 5: 動作確認

```bash
# Gateway の NodeInfo を確認
curl http://<IP>:3000/.well-known/title-node-info

# Indexer の死活確認
curl http://<IP>:5000/health
```

## ノードの停止

```bash
# Enclave の停止
sudo nitro-cli terminate-enclave --all

# 全サービスの停止
cd ~/title-protocol
docker compose -f deploy/aws/docker-compose.production.yml down

# Proxy の停止
pkill title-proxy || true
```

## ノードの再起動

TEE はステートレスなので、再起動すると鍵が再生成される。

```bash
# 再デプロイ
./deploy/aws/setup-ec2.sh
```

再起動後は GlobalConfig の更新が必要。`setup-ec2.sh` の Step 7 で
`init-devnet.mjs` が自動実行されるが、手動でも可能:

```bash
cd scripts
node init-devnet.mjs --rpc $SOLANA_RPC_URL --gateway http://localhost:3000
```

GlobalConfig の運用手順の詳細は `docs/v1/GLOBALCONFIG-GUIDE.md` を参照。

## トラブルシューティング

| 症状 | 原因 | 対処 |
|------|------|------|
| `docker: permission denied` | Docker グループ未反映 | `exit` → 再 SSH、または `sg docker bash` |
| `cargo build` で C コンパイラ不在 | user-data.sh 未完了 | `sudo dnf install -y gcc gcc-c++` |
| Enclave 起動失敗 | メモリ不足 | `enclave_memory_mib` を調整。`/etc/nitro_enclaves/allocator.yaml` も確認 |
| S3 presigned URL が 403 | IAM キー未設定 | `terraform output` で S3 キーを取得して .env に設定 |
| Gateway が起動しない | ポート競合 | `ss -tlnp \| grep 3000` で確認。既存プロセスを停止 |
| `solana airdrop` 失敗 | devnet レート制限 | https://faucet.solana.com/ で手動取得 |
| `cargo-build-sbf` edition2024 エラー | Platform Tools が古い | `--tools-version v1.52` を指定 |

## ファイル構成

```
deploy/aws/
  terraform/
    main.tf             — EC2, S3, IAM, SecurityGroup 定義
    variables.tf        — 設定変数
    outputs.tf          — IP, S3キー等の出力値
    user-data.sh        — EC2 初期化スクリプト（Terraform から注入）
  docker/
    tee.Dockerfile      — TEE Enclave 用マルチステージ Docker イメージ
    entrypoint.sh       — Enclave 内起動スクリプト（socat + TEE サーバー）
  docker-compose.production.yml  — 本番用 Compose（Gateway + Indexer + PostgreSQL）
  setup-ec2.sh          — メインデプロイスクリプト（全 8 ステップ）
  build-enclave.sh      — EIF 単体ビルドスクリプト
  keys/                 — SSH 鍵、Authority keypair（.gitignore 済み）
```
