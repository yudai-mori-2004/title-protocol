# AWS Node Deployment

Deploy a Title Protocol node on AWS with Nitro Enclaves. One EC2 instance = one TEE node. Idempotent — safe to re-run.

> For local development, see [`deploy/local/README.md`](../local/README.md).

---

## Prerequisites

| Tool | Notes |
|------|-------|
| [AWS CLI](https://aws.amazon.com/cli/) configured | `aws configure` |
| [Terraform](https://www.terraform.io/) 1.5+ | |
| `network.json` | Created by Phase 1 (see [`programs/title-config/README.md`](../../programs/title-config/README.md)) |
| ~0.6 SOL on devnet | For node registration + Merkle Tree creation |

Phase 1 (program deploy + GlobalConfig init) must be completed first. See [QUICKSTART.md](../../QUICKSTART.md) or [`deploy/local/README.md`](../local/README.md).

---

## Step 1: Create Infrastructure

```bash
# Create SSH key pair (skip if you already have one)
mkdir -p deploy/aws/keys
aws ec2 create-key-pair \
  --key-name title-protocol-devnet \
  --query 'KeyMaterial' \
  --output text > deploy/aws/keys/title-protocol-devnet.pem
chmod 400 deploy/aws/keys/title-protocol-devnet.pem

# Provision infrastructure
cd deploy/aws/terraform
terraform init
terraform apply
cd ../../..
```

Terraform creates everything from scratch:

| Resource | Purpose |
|----------|---------|
| EC2 (c5.xlarge) | Nitro Enclave capable. Amazon Linux 2023. ~$0.10/hr |
| Elastic IP | Fixed public IP per node |
| S3 bucket | Encrypted content temp storage (auto-expires after 1 day, shared across nodes) |
| IAM user + access key | S3 authentication for Gateway |
| Security Group | Inbound SSH (22) and Gateway (3000) |

To scale to multiple nodes:

```bash
terraform apply -var="node_count=3"   # scale up
terraform apply -var="node_count=2"   # scale down (last node removed)
```

Each node registers independently on-chain. The SDK discovers available nodes automatically.

---

## Step 2: Configure `.env`

Get the S3 credentials from Terraform output and set them in `.env`:

```bash
cd deploy/aws/terraform
terraform output nodes                     # Node IPs + SSH commands
terraform output -raw s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output -raw s3_bucket_name
cd ../../..
```

| `.env` variable | Source |
|-----------------|--------|
| `SOLANA_RPC_URL` | Already in `.env.example` (change for dedicated RPC) |
| `S3_ENDPOINT` | `terraform output -raw s3_bucket_endpoint` |
| `S3_BUCKET` | `terraform output -raw s3_bucket_name` |
| `S3_ACCESS_KEY` | `terraform output -raw s3_access_key_id` |
| `S3_SECRET_KEY` | `terraform output -raw s3_secret_access_key` |

> All environment variables: [docs/reference.md](../../docs/reference.md)

---

## Step 3: Deploy Node

SSH into the instance, clone the repo, and run the setup script.

```bash
# SSH in (replace NODE_IP with Elastic IP from terraform output)
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP

# --- on EC2 ---
git clone https://github.com/yudai-mori-2004/title-protocol.git ~/title-protocol
cd ~/title-protocol
cp .env.example .env
vim .env   # Set S3_ENDPOINT, S3_BUCKET, S3_ACCESS_KEY, S3_SECRET_KEY
```

Copy keys from your local machine:

```bash
# From local machine:
scp -i deploy/aws/keys/title-protocol-devnet.pem \
  keys/authority.json keys/operator.json network.json \
  ec2-user@NODE_IP:~/title-protocol/keys/

# Move network.json to project root
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP \
  "mv ~/title-protocol/keys/network.json ~/title-protocol/network.json"
```

Run the setup:

```bash
# On EC2:
cd ~/title-protocol
./deploy/aws/setup-ec2.sh
```

First run builds everything from source (WASM, Rust, Docker, EIF). Expect **20-40 minutes**. Subsequent runs use cached builds.

What `setup-ec2.sh` does:

| Step | Action |
|------|--------|
| 0 | Check config (.env, network.json, keys, SOL balance) |
| 1 | Build 4 WASM modules |
| 2 | Build host binaries (Proxy or TEE, CLI) |
| 3 | Build Enclave image (Docker → `nitro-cli build-enclave` → EIF) |
| 4 | Start TEE (Nitro Enclave + socat bridge on TCP:4000) |
| 5 | Start Proxy (vsock:8000 → external HTTP for Solana RPC / Arweave) |
| 6 | Start Gateway via Docker Compose (:3000) |
| 7 | Verify S3 bucket access |
| 8 | Register TEE node on-chain (auto-signs if `keys/authority.json` exists) |
| 9 | Create Merkle Trees (Core + Extension) |
| 10 | Health check all services |

Values auto-configured by `setup-ec2.sh` (no manual setup needed):

| Value | Source |
|-------|--------|
| `GATEWAY_SIGNING_KEY` | Auto-generated, appended to `.env` |
| `GLOBAL_CONFIG_PDA` | Read from `network.json`, appended to `.env` |
| `CORE_COLLECTION_MINT` | Read from `network.json` |
| `EXT_COLLECTION_MINT` | Read from `network.json` |
| `PUBLIC_ENDPOINT` | Auto-detected from EC2 metadata (IMDSv2) |
| `keys/operator.json` | Copied from `~/.config/solana/id.json`, or auto-generated |

---

## Verify the Node

From your local machine:

```bash
# Build the SDK
cd sdk/ts && npm install && npm run build && cd ../..

# Run verification (replace NODE_IP with the Elastic IP)
cd integration-tests && npm install
npx tsx register-photo.ts NODE_IP ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --skip-sign
```

You should see `tee_type: aws_nitro` in the output, confirming real Nitro Enclave verification.

For the full flow (verify + Arweave upload + cNFT mint):

```bash
npx tsx register-photo.ts NODE_IP ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --broadcast
```

---

## Devnet vs Mainnet

The presence of `keys/authority.json` determines behavior:

| | Devnet (your own GlobalConfig) | Mainnet (DAO GlobalConfig) |
|---|---|---|
| `keys/authority.json` | Exists locally (created by init-global) | **Does not exist** (DAO-controlled) |
| Node registration | Auto co-signs and broadcasts immediately | Outputs partial TX for DAO approval |

### Running a Mainnet Node

Node operators do **not** run Phase 1 — the DAO has already deployed the program and initialized GlobalConfig.

**1. Get `network.json`** — Download from the protocol's public repository or DAO website. It contains the mainnet Program ID, GlobalConfig PDA, and collection mints.

**2. Deploy your node** — Same steps as above, with these differences:
- Use the mainnet `network.json` (not your own)
- Set `SOLANA_RPC_URL` to a mainnet RPC endpoint in `.env`
- Do **not** copy `keys/authority.json` (the DAO controls it)

**3. Submit registration for DAO approval** — Since `keys/authority.json` is absent, `setup-ec2.sh` outputs a partially-signed transaction. Send it to the DAO via the designated governance channel. Once co-signed and broadcast, your node is registered.

**4. Create Merkle Trees** — After registration is confirmed:

```bash
./target/release/title-cli create-tree \
  --tee-url http://localhost:4000 \
  --max-depth 14 --max-buffer-size 64
```

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

## Stop

```bash
sudo nitro-cli terminate-enclave --all
docker compose -f deploy/aws/docker-compose.production.yml down
pkill title-proxy || true
```

---

## Restart

TEE nodes are stateless — keys are regenerated on each restart and the node re-registers automatically.

```bash
./deploy/aws/setup-ec2.sh
```

---

## Teardown

To destroy all AWS resources:

```bash
cd deploy/aws/terraform
terraform destroy
```

---

## What's Next

| Goal | Guide |
|------|-------|
| Understand the architecture | [docs/architecture.md](../../docs/architecture.md) |
| Run locally instead | [deploy/local/README.md](../local/README.md) |
| Build an app with the SDK | [sdk/ts/README.md](../../sdk/ts/README.md) |
| Query indexed cNFTs | [indexer/README.md](../../indexer/README.md) |
| Environment variables & CLI reference | [docs/reference.md](../../docs/reference.md) |
| Troubleshooting | [docs/troubleshooting.md](../../docs/troubleshooting.md) |
