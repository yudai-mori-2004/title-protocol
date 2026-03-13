# deploy/aws — AWS Nitro Enclave Node Deployment

Title Protocol ノードを AWS 上で起動する完全手順。
1 インスタンス = 1 TEE = 1 エンドポイント。冪等に何度でも実行可能。

> ローカル開発は [`deploy/local/README.md`](../local/README.md) を参照。

---

## Architecture

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

---

## Prerequisites

| Tool | Notes |
|------|-------|
| AWS CLI configured | `aws configure` |
| Terraform 1.5+ | |
| SSH key pair | For EC2 access |
| `network.json` | Phase 1 で作成。→ [`programs/title-config/README.md`](../../programs/title-config/README.md) |
| ~0.6 SOL on devnet | ノード登録 + Merkle Tree 作成に必要 |

---

## Step 1: Terraform Infrastructure

```bash
# SSH キーペアの準備（既存があればスキップ）
mkdir -p deploy/aws/keys
aws ec2 create-key-pair \
  --key-name title-protocol-devnet \
  --query 'KeyMaterial' \
  --output text > deploy/aws/keys/title-protocol-devnet.pem
chmod 400 deploy/aws/keys/title-protocol-devnet.pem

# Terraform
cd deploy/aws/terraform
terraform init
terraform apply                          # 1ノード（デフォルト）
# terraform apply -var="node_count=3"    # 3ノードに拡張
cd ../../..
```

### Created Resources

| Resource | Purpose |
|----------|---------|
| EC2 (c5.xlarge) × node_count | Nitro Enclave 対応。Amazon Linux 2023。~$0.10/hr |
| Elastic IP × node_count | ノードごとの固定パブリック IP |
| S3 bucket | 暗号化コンテンツの一時保管（1日で自動削除、全ノード共有） |
| IAM user + access key | Gateway の S3 認証用 |
| Security Group | SSH:22, Gateway:3000 |

### Scaling

```bash
# ノードを3台に拡張（既存ノードは影響なし）
terraform apply -var="node_count=3"

# 2台に縮小（最後のノードが削除される）
terraform apply -var="node_count=2"
```

Each node registers independently on-chain and operates as a separate TEE. The SDK's `selectNode()` automatically discovers available nodes.

---

## Step 2: Configure `.env`

```bash
# Get Terraform outputs
cd deploy/aws/terraform
terraform output nodes                     # All node IPs + SSH commands
terraform output -raw s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output -raw s3_bucket_name
cd ../../..
```

**Terraform output → `.env` mapping:**

| `.env` variable | Terraform command | Notes |
|-----------------|-------------------|-------|
| `SOLANA_RPC_URL` | *(already in .env.example)* | Change for dedicated RPC |
| `S3_ENDPOINT` | `terraform output -raw s3_bucket_endpoint` | e.g. `https://s3.ap-northeast-1.amazonaws.com` |
| `S3_BUCKET` | `terraform output -raw s3_bucket_name` | e.g. `title-uploads-devnet` |
| `S3_ACCESS_KEY` | `terraform output -raw s3_access_key_id` | |
| `S3_SECRET_KEY` | `terraform output -raw s3_secret_access_key` | |

> 全環境変数の詳細は [docs/reference.md](../../docs/reference.md) を参照。

---

## Step 3: Deploy Node

**For each node**, SSH in, clone the repo, configure, and deploy:

```bash
# SSH into the node (replace NODE_IP with Elastic IP from terraform output)
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP

# --- on EC2 ---
git clone https://github.com/yudai-mori-2004/title-protocol.git ~/title-protocol
cd ~/title-protocol
cp .env.example .env
vim .env  # Set S3_ENDPOINT, S3_BUCKET, S3_ACCESS_KEY, S3_SECRET_KEY
```

### Copy Keys from Local

```bash
# From local machine:
scp -i deploy/aws/keys/title-protocol-devnet.pem \
  keys/authority.json keys/operator.json network.json \
  ec2-user@NODE_IP:~/title-protocol/keys/

# network.json はルートに配置
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP \
  "mv ~/title-protocol/keys/network.json ~/title-protocol/network.json"
```

### Run Setup

```bash
# On EC2:
cd ~/title-protocol
./deploy/aws/setup-ec2.sh
```

> **First run:** Builds WASM modules, Rust binaries, Docker images, and a Nitro Enclave EIF. Expect **20-40 minutes** on first run.

### What `setup-ec2.sh` Does

| Step | What | Details |
|------|------|---------|
| 0 | Config check | .env, network.json, keys, SOL balance |
| 1 | Build WASM modules | 4 modules → `wasm-modules/` |
| 2 | Build host binaries | Proxy (or TEE for mock), CLI |
| 3 | Build EIF | Docker → `nitro-cli build-enclave` → `title-tee.eif` |
| 4 | Start TEE | Nitro Enclave + socat inbound bridge (TCP:4000 → vsock) |
| 5 | Start Proxy | vsock:8000 → external HTTP (Solana RPC, Arweave) |
| 6 | Docker Compose | Gateway (:3000) |
| 7 | S3 check | Verify bucket access |
| 8 | Register TEE node | オンチェーン登録（auto-sign or partial TX） |
| 9 | Create Merkle Trees | Core + Extension trees |
| 10 | Health check | 全サービスの応答確認 |

### Auto-configured Values

| Value | Source |
|-------|--------|
| `GATEWAY_SIGNING_KEY` | 自動生成 + `.env` に追記 |
| `GLOBAL_CONFIG_PDA` | `network.json` → `.env` に追記 |
| `CORE_COLLECTION_MINT` | `network.json` から自動読み取り |
| `EXT_COLLECTION_MINT` | `network.json` から自動読み取り |
| `PUBLIC_ENDPOINT` | EC2 メタデータ (IMDSv2) から公開 IP を自動取得 |
| `keys/operator.json` | `~/.config/solana/id.json` からコピー、または自動生成 |

---

## Devnet vs Mainnet

`keys/authority.json` の有無で挙動が変わる:

| | Devnet（自前 GlobalConfig） | Mainnet（公式 GlobalConfig） |
|---|---|---|
| `keys/authority.json` | ローカルに存在（init-global で作成） | **存在しない**（DAO 管理） |
| Step 8: register-node | 自動で共同署名 → 即ブロードキャスト | 部分署名 TX を表示 → DAO に審査依頼 |

### Running a Mainnet Node

Node operators do **not** run Phase 1 — the DAO has already deployed the program and initialized GlobalConfig. You only need Phase 2.

**1. Get `network.json`**

Download the canonical `network.json` from the protocol's public repository or DAO website. This contains the mainnet Program ID, GlobalConfig PDA, and collection mints.

**2. Deploy your node**

Follow the same steps above with these differences:

- Use the mainnet `network.json` (not your own)
- Set `SOLANA_RPC_URL` to a mainnet RPC endpoint in `.env`
- Do **not** copy `keys/authority.json` (you don't have it — the DAO controls it)

```bash
# On EC2:
./deploy/aws/setup-ec2.sh
```

**3. Submit registration transaction for DAO approval**

Since `keys/authority.json` is absent, `setup-ec2.sh` outputs a partially-signed transaction:

```
TEEノード登録: authority.json が見つかりません
以下のトランザクションを authority に署名・送信してください:
<base64-encoded partial transaction>
```

Send this to the DAO via the designated channel (governance proposal, multi-sig queue). Once the DAO co-signs and broadcasts, your node is registered.

**4. Create Merkle Trees**

After registration is confirmed:

```bash
./target/release/title-cli create-tree --tee-url http://localhost:4000 --max-depth 14 --max-buffer-size 64
```

This also requires DAO co-signature if the tree rent payer differs from the authority.

---

## Logs

```bash
# Gateway (Docker)
docker compose -f deploy/aws/docker-compose.production.yml logs -f

# TEE (Nitro Enclave console)
sudo nitro-cli console --enclave-id $(nitro-cli describe-enclaves | \
  python3 -c "import sys,json; print(json.load(sys.stdin)[0]['EnclaveID'])")

# Proxy
tail -f ~/title-proxy.log
```

---

## Quick Test

Verify a C2PA-signed photo through the Nitro Enclave:

```bash
# From your local machine (replace NODE_IP with the Elastic IP):
cd integration-tests && npm install
npx tsx register-photo.ts NODE_IP ./fixtures/pixel_photo_ramen.jpg \
  --wallet keys/operator.json --skip-sign
```

You should see `tee_type: aws_nitro` in the output, confirming real Nitro Enclave verification.

---

## Stop

```bash
# Enclave
sudo nitro-cli terminate-enclave --all

# Gateway
docker compose -f deploy/aws/docker-compose.production.yml down

# Proxy
pkill title-proxy || true
```

---

## Restart

TEE nodes are stateless. On restart, keys are regenerated and the node re-registers.

```bash
./deploy/aws/setup-ec2.sh
```

---

## File Structure

```
deploy/aws/
  terraform/           — EC2, EIP, S3, IAM, SecurityGroup (Terraform)
  docker/              — tee.Dockerfile, gateway.Dockerfile, entrypoint.sh
  docker-compose.production.yml  — Gateway
  setup-ec2.sh         — ノード起動スクリプト（冪等、EIF ビルド含む）
  keys/                — SSH 鍵（.gitignore 済み）

keys/
  authority.json       — Authority keypair（init-global で作成、gitignore 済み）
  operator.json        — Operator wallet（setup.sh が自動作成、gitignore 済み）

network.json           — GlobalConfig 情報（init-global で作成、gitignore 済み）
```

---

## Troubleshooting

See [docs/troubleshooting.md](../../docs/troubleshooting.md).
