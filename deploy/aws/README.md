# AWS Nitro Enclave Node Deployment Guide

Title Protocol ノードを AWS 上で起動する手順書。
1インスタンス = 1 TEE = 1エンドポイント。冪等に何度でも実行可能。

## 前提

- `network.json` が存在する（`scripts/init-global.mjs` で事前に作成済み）
- AWS アカウント（AdministratorAccess 権限推奨）
- AWS CLI 設定済み（`aws configure`）
- Terraform 1.5+

## アーキテクチャ

```
                    +-- EC2 Instance (c5.xlarge) ----------------------+
                    |                                                   |
Internet --:3000->  |  Docker Compose                                   |
                    |  +----------+  +----------+  +----------+        |
                    |  | Gateway  |  | Indexer   |  | Postgres |        |
                    |  +----+-----+  +----------+  +----------+        |
                    |       | :4000                                     |
                    |       v                                           |
                    |  socat (TCP:4000 <-> vsock)                       |
                    |       |                                           |
                    |  +----v----------------------------+              |
                    |  |  Nitro Enclave (EIF)            |              |
                    |  |  +---------+  +------------+   |              |
                    |  |  | TEE     |  | WASM       |   |              |
                    |  |  | Server  |  | Modules x4 |   |              |
                    |  |  +----+----+  +------------+   |              |
                    |  |       | :8000                   |              |
                    |  |  socat (vsock <-> TCP:8000)     |              |
                    |  +-------+------------------------+              |
                    |           v                                       |
                    |  title-proxy (HTTP <-> Solana RPC / Arweave)     |
                    |                                                   |
                    |  S3 (Temp Storage)                                |
                    +---------------------------------------------------+
```

## Step 1: Terraform でインフラ作成

```bash
# SSH キーペアの準備
mkdir -p deploy/aws/keys
aws ec2 create-key-pair \
  --key-name title-protocol-devnet \
  --query 'KeyMaterial' \
  --output text > deploy/aws/keys/title-protocol-devnet.pem
chmod 400 deploy/aws/keys/title-protocol-devnet.pem

# Terraform
cd deploy/aws/terraform
terraform init
terraform plan
terraform apply
```

### 作成されるリソース

| リソース | 用途 |
|---------|------|
| EC2 (c5.xlarge) | Nitro Enclave 対応。Amazon Linux 2023 |
| S3 バケット | 暗号化コンテンツの一時保管（1日で自動削除） |
| IAM ユーザー + アクセスキー | Gateway の S3 認証用 |
| Security Group | SSH:22, Gateway:3000, Indexer:5000 |

## Step 2: .env の設定

```bash
# Terraform output から値を取得
terraform output instance_public_ip
terraform output s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output s3_bucket_name
```

## Step 3: ノード起動

```bash
# SSH
ssh -i deploy/aws/keys/title-protocol-devnet.pem \
  ec2-user@$(terraform output -raw instance_public_ip)

# EC2 上で
git clone <REPO_URL> ~/title-protocol
cd ~/title-protocol
cp .env.example .env
vim .env  # Terraform output の値を設定

# ノード起動（全自動）
./deploy/aws/setup-ec2.sh
```

### setup-ec2.sh のステップ

| Step | 内容 |
|------|------|
| 0 | .env + network.json の読み込みと検証 |
| 1 | WASM モジュール 4 個のビルド |
| 2 | ホスト側バイナリのビルド |
| 3 | Enclave イメージ (EIF) のビルド |
| 4 | TEE の起動（Enclave or MockRuntime） |
| 5 | Proxy の起動（Enclaveモードのみ） |
| 6 | Docker Compose（Gateway + Indexer + PostgreSQL） |
| 7 | S3 アクセスの検証 |
| 8 | TEEノード登録（/register-node → DAO署名） |
| 9 | Merkle Tree 作成（/create-tree） |
| 10 | ヘルスチェック |

### Devnet vs Mainnet の違い

`programs/title-config/keys/authority.json` が存在するかどうかのみ。

| | Devnet（自前GlobalConfig） | Mainnet（公式GlobalConfig） |
|---|---|---|
| `programs/title-config/keys/authority.json` | 存在する | 存在しない |
| Step 8 | 自動で共同署名 → 即ブロードキャスト | 部分署名TXを表示 → DAOに審査依頼 |

## ノードの停止

```bash
# Enclave の停止
sudo nitro-cli terminate-enclave --all

# 全サービスの停止
docker compose -f deploy/aws/docker-compose.production.yml down

# Proxy の停止
pkill title-proxy || true
```

## ノードの再起動

TEE はステートレス。再起動すると鍵が再生成される。

```bash
./deploy/aws/setup-ec2.sh
```

## トラブルシューティング

| 症状 | 対処 |
|------|------|
| `docker: permission denied` | `exit` → 再 SSH、または `sg docker bash` |
| `cargo build` で C コンパイラ不在 | `sudo dnf install -y gcc gcc-c++` |
| Enclave 起動失敗 | `enclave_memory_mib` を調整 |
| S3 presigned URL が 403 | Terraform output で S3 キーを再確認 |
| `network.json が見つかりません` | `scripts/init-global.mjs` を先に実行 |

## ファイル構成

```
deploy/aws/
  terraform/           — EC2, S3, IAM, SecurityGroup (Terraform)
  docker/              — tee.Dockerfile, entrypoint.sh
  docker-compose.production.yml  — Gateway + Indexer + PostgreSQL
  setup-ec2.sh         — ノード起動スクリプト（冪等、EIFビルド含む）
  keys/                — SSH 鍵（.gitignore 済み）

keys/                  — Authority keypair（.gitignore 済み）
network.json           — GlobalConfig 情報（リポジトリにコミット）
```
