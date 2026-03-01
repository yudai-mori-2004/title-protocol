# AWS Nitro Enclave Node Deployment Guide

Title Protocol ノードを AWS 上で起動する手順書。
1インスタンス = 1 TEE = 1エンドポイント。冪等に何度でも実行可能。
`node_count` を増やすだけで複数ノードを並列デプロイできる。

## 前提

- `network.json` が存在する（`title-cli init-global` で事前に作成済み）
- AWS アカウント（AdministratorAccess 権限推奨）
- AWS CLI 設定済み（`aws configure`）
- Terraform 1.5+

## アーキテクチャ

```
                    +-- EC2 Instance (c5.xlarge) ----------------------+
                    |                              [Elastic IP]        |
Internet --:3000->  |  Docker Compose                                   |
                    |  +----------+                                    |
                    |  | Gateway  |                                    |
                    |  +----+-----+                                    |
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
                    |  S3 (Temp Storage) — shared across all nodes     |
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
terraform apply                          # 1ノード（デフォルト）
# terraform apply -var="node_count=3"    # 3ノードに拡張
```

### 作成されるリソース

| リソース | 用途 |
|---------|------|
| EC2 (c5.xlarge) × node_count | Nitro Enclave 対応。Amazon Linux 2023 |
| Elastic IP × node_count | ノードごとの固定パブリックIP |
| S3 バケット | 暗号化コンテンツの一時保管（1日で自動削除、全ノード共有） |
| IAM ユーザー + アクセスキー | Gateway の S3 認証用 |
| Security Group | SSH:22, Gateway:3000 |

### スケーリング

```bash
# ノードを3台に拡張（既存ノードは影響なし）
terraform apply -var="node_count=3"

# 2台に縮小（最後のノードが削除される）
terraform apply -var="node_count=2"
```

各ノードは独立したTEEとして動作する。`setup-ec2.sh` を実行するだけで自動的に GlobalConfig に登録され、SDKの `selectNode()` が利用可能なノードを自動発見する。

## Step 2: .env の設定

```bash
# Terraform output から値を取得
terraform output nodes                     # 全ノードのIP + SSH コマンド
terraform output s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output s3_bucket_name
```

## Step 3: ノード起動

各ノードに SSH してセットアップを実行:

```bash
# SSH (terraform output nodes の ssh_command を使用)
ssh -i deploy/aws/keys/title-protocol-devnet.pem \
  ec2-user@NODE_IP

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
| 6 | Docker Compose（Gateway） |
| 7 | S3 アクセスの検証 |
| 8 | TEEノード登録（/register-node → DAO署名） |
| 9 | Merkle Tree 作成（/create-tree） |
| 10 | ヘルスチェック |

### Devnet vs Mainnet の違い

`keys/authority.json` が存在するかどうかで挙動が変わる。

| | Devnet（自前GlobalConfig） | Mainnet（公式GlobalConfig） |
|---|---|---|
| `keys/authority.json` | ローカルに存在（init-global で作成） | 存在しない（DAO管理） |
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
| `network.json が見つかりません` | `title-cli init-global` を先に実行 |
| `solana: command not found` | `source ~/.bashrc` または新しい SSH セッションを開く |

## ファイル構成

```
deploy/aws/
  terraform/           — EC2, EIP, S3, IAM, SecurityGroup (Terraform)
  docker/              — tee.Dockerfile, gateway.Dockerfile, entrypoint.sh
  docker-compose.production.yml  — Gateway
  setup-ec2.sh         — ノード起動スクリプト（冪等、EIFビルド含む）
  keys/                — SSH 鍵（.gitignore 済み）

keys/
  authority.json       — Authority keypair（init-global で作成、gitignore 済み）
  operator.json        — Operator wallet（setup.sh が自動作成、gitignore 済み）

network.json           — GlobalConfig 情報（init-global で作成、gitignore 済み）
```
