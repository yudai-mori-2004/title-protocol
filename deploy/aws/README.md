# AWS Node Deployment

Deploy a Title Protocol TEE node on AWS Nitro Enclaves. One EC2 instance = one node.

> For local development, see [`deploy/local/README.md`](../local/README.md).

---

## Architecture

```
Internet → Gateway (:3000) → socat bridge (:4000) → Nitro Enclave (vsock)
                                                          ↕
                                                    title-proxy (vsock:8000)
                                                          ↓
                                                    Solana RPC / Arweave
```

- **Nitro Enclave**: Isolated VM running `title-tee`. Cannot access host network, disk, or processes. All external communication goes through `title-proxy` via vsock.
- **Gateway**: HTTP server on the host. Receives client requests, forwards to Enclave via socat, manages S3 temp storage.
- **Proxy**: Runs on the host. Bridges Enclave's vsock to external HTTPS (Solana RPC, storage).

### Stateless Design

TEE nodes are **stateless**. Every restart:
1. New Ed25519 signing key + X25519 encryption key generated in Enclave memory
2. Node re-registers on-chain with the new keys
3. Old on-chain node entry is replaced (same Enclave, new keys)

No persistent state to back up or migrate.

### Memory Model

Nitro Enclaves reserve **hugepages** from host RAM. The reservation size is `ENCLAVE_MEMORY_MIB` in `.env` (default: 3072). The remainder is available for the host OS, builds, and services. Choose an instance type with enough total RAM for both the Enclave and build processes.

Hugepages are a hard reservation — they reduce available host memory even when the Enclave is not running. `setup-ec2.sh` automatically releases hugepages before builds and re-reserves them before starting the Enclave, preventing OOM during compilation.

### WASM Module Loading

WASM modules (PDQ, vPDQ, certificate verifiers) run inside the Enclave. Two loading modes:

| Mode | Config | Behavior |
|------|--------|----------|
| **FileLoader** | `WASM_DIR=/wasm-modules` in `.env` | Modules baked into Enclave image at build time. No external fetch. |
| **ConfigLoader** | `WASM_DIR` unset | Resolves URIs from GlobalConfig PDA, fetches via HTTPS. |

FileLoader is simpler and has no runtime dependency on external storage. WASM hashes are registered on-chain for client-side verification regardless of loading mode.

---

## Prerequisites

| Requirement | Notes |
|-------------|-------|
| [AWS CLI](https://aws.amazon.com/cli/) configured | `aws configure` |
| [Terraform](https://www.terraform.io/) 1.5+ | |
| `network.json` | Created by Phase 1 (see [`programs/title-config/README.md`](../../programs/title-config/README.md)) |
| ~0.6 SOL on devnet | Node registration + Merkle Tree creation |

Phase 1 (program deploy + GlobalConfig init) must be completed first. See [QUICKSTART.md](../../QUICKSTART.md).

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

Terraform creates:

| Resource | Purpose |
|----------|---------|
| EC2 (c5.xlarge) | Nitro Enclave capable. Amazon Linux 2023. ~$0.10/hr |
| Elastic IP | Fixed public IP per node |
| S3 bucket | Encrypted content temp storage (auto-expires 1 day) |
| IAM user + access key | S3 authentication for Gateway |
| Security Group | Inbound SSH (22) and Gateway (3000) |

To scale:

```bash
terraform apply -var="node_count=3"   # scale up
terraform apply -var="node_count=2"   # scale down
```

---

## Step 2: Configure `.env`

```bash
cd deploy/aws/terraform
terraform output nodes                     # Node IPs + SSH commands
terraform output -raw s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output -raw s3_bucket_name
terraform output -raw signed_json_s3_bucket_name
cd ../../..
```

| `.env` variable | Source |
|-----------------|--------|
| `SOLANA_RPC_URL` | Already in `.env.example` (change for dedicated RPC) |
| `S3_ENDPOINT` | `terraform output -raw s3_bucket_endpoint` |
| `S3_BUCKET` | `terraform output -raw s3_bucket_name` |
| `S3_ACCESS_KEY` | `terraform output -raw s3_access_key_id` |
| `S3_SECRET_KEY` | `terraform output -raw s3_secret_access_key` |
| `S3_REGION` | AWS region (e.g. `ap-northeast-1`) |
| `SIGNED_JSON_S3_BUCKET` | `terraform output -raw signed_json_s3_bucket_name` |
| `WASM_DIR` | `/wasm-modules` (FileLoader, recommended) |

---

## Step 3: Deploy Node

```bash
# SSH in (replace NODE_IP with Elastic IP from terraform output)
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP

# --- on EC2 ---
git clone https://github.com/yudai-mori-2004/title-protocol.git ~/title-protocol
cd ~/title-protocol
cp .env.example .env
vim .env   # Set S3 credentials + WASM_DIR=/wasm-modules
```

Copy keys from your local machine:

```bash
# From local machine:
scp -i deploy/aws/keys/title-protocol-devnet.pem \
  network.json ec2-user@NODE_IP:~/title-protocol/

# authority.json is required for devnet (auto-sign mode)
scp -i deploy/aws/keys/title-protocol-devnet.pem \
  keys/authority.json ec2-user@NODE_IP:~/title-protocol/keys/
```

`keys/operator.json` is optional — `setup-ec2.sh` auto-creates one if missing. Fund it with SOL when prompted.

Run the setup:

```bash
cd ~/title-protocol
./deploy/aws/setup-ec2.sh
```

First run: **20-40 minutes** (builds everything from source). Subsequent runs: **5-10 minutes** (cached builds).

### What `setup-ec2.sh` does

| Step | Action |
|------|--------|
| 0 | Validate config (.env, network.json, keys, SOL balance) |
| 1 | Build WASM modules |
| 1.5 | Release hugepages for build memory |
| 2 | Build host binaries (Proxy, CLI) |
| 3 | Build Enclave image (Docker → EIF), reserve hugepages |
| 4 | Start Enclave + socat bridge |
| 5 | Start Proxy |
| 6 | Start Gateway (Docker Compose) |
| 7 | Verify S3 access |
| 8 | Register node on-chain |
| 9 | Create Merkle Trees |
| 10 | Create Address Lookup Table |
| 11 | Health check |

Auto-configured values (no manual setup needed):

| Value | Source |
|-------|--------|
| `GATEWAY_SIGNING_KEY` | Random, appended to `.env` |
| `GATEWAY_SOLANA_KEYPAIR` | From `keys/operator.json` |
| `GLOBAL_CONFIG_PDA` | From `network.json` |
| `PUBLIC_ENDPOINT` | EC2 metadata (IMDSv2) |

---

## Step 4: Register WASM Hashes

After `setup-ec2.sh` completes, register the WASM module hashes on GlobalConfig so clients can verify WASM integrity. Run from your **local machine** (requires `keys/authority.json`):

```bash
# Copy WASM binaries from EC2
mkdir -p /tmp/ec2-wasm
scp -i deploy/aws/keys/title-protocol-devnet.pem \
  ec2-user@NODE_IP:~/title-protocol/wasm-modules/*.wasm /tmp/ec2-wasm/

# Register each module's hash on-chain
for f in /tmp/ec2-wasm/*.wasm; do
  name=$(basename "$f" .wasm)
  ./target/release/title-cli add-wasm-version \
    --extension-id "$name" \
    --wasm-path "$f" \
    --wasm-source local
done
```

This registers the SHA-256 hash of each WASM binary in GlobalConfig. The TEE loads modules locally (FileLoader), but clients verify the hash against on-chain data.

---

## Verify

From your local machine using the SDK:

```typescript
import { fetchGlobalConfig, TitleClient } from "@title-protocol/sdk";

const config = await fetchGlobalConfig("devnet");
const client = new TitleClient(config);
const result = await client.register({
  content: imageBuffer,
  ownerWallet: "YourWallet...",
  processorIds: ["core-c2pa", "image-pdq"],
  delegateMint: true,
  gatewayEndpoint: "http://NODE_IP:3000",
});
```

You should see `tee_type: aws_nitro` in the signed result, confirming Nitro Enclave execution.

---

## Update

After code changes (pull new commits, update WASM modules, etc.):

```bash
# On EC2:
cd ~/title-protocol
git pull
./deploy/aws/setup-ec2.sh
```

The script is idempotent:
- Stops existing Enclave and releases hugepages
- Rebuilds only changed components (incremental cargo/docker builds)
- Re-reserves hugepages and starts new Enclave
- Re-registers node with new signing key

If WASM modules changed, re-run Step 4 from your local machine to update on-chain hashes.

**Node re-registration**: Each restart generates new keys. The old on-chain node entry accumulates stale entries. Clean up with:

```bash
# From local machine:
title-cli remove-node --signing-pubkey <OLD_PUBKEY>
```

Use the SDK's `fetchGlobalConfig("devnet")` to list current nodes and identify stale entries.

---

## Devnet vs Mainnet

| | Devnet | Mainnet |
|---|---|---|
| `keys/authority.json` | Present (you control GlobalConfig) | Absent (DAO controls) |
| Node registration | Auto co-signs immediately | Outputs partial TX for DAO approval |
| WASM registration | You run `add-wasm-version` | DAO runs it |

### Mainnet Node Operators

1. **Get `network.json`** from the protocol repository (contains program ID, GlobalConfig PDA, collection mints)
2. **Deploy** — same steps, but use mainnet `network.json` and `SOLANA_RPC_URL`, omit `authority.json`
3. **Submit registration** — `setup-ec2.sh` outputs a partial TX. Send to DAO for co-signing.
4. **Create Merkle Trees** after registration is confirmed:
   ```bash
   ./target/release/title-cli create-tree --tee-url http://localhost:4000 --max-depth 14 --max-buffer-size 64
   ```
5. **Create ALT** after trees are created:
   ```bash
   ./target/release/title-cli create-alt --tee-url http://localhost:4000
   ```

---

## Operations

### Logs

```bash
# Gateway
docker compose -f deploy/aws/docker-compose.production.yml logs -f

# TEE (Enclave console)
sudo nitro-cli console --enclave-id $(nitro-cli describe-enclaves | \
  python3 -c "import sys,json; print(json.load(sys.stdin)[0]['EnclaveID'])")

# Proxy
tail -f ~/title-proxy.log
```

### Stop

```bash
sudo nitro-cli terminate-enclave --all
docker compose -f deploy/aws/docker-compose.production.yml down
pkill title-proxy || true
```

### Restart

```bash
./deploy/aws/setup-ec2.sh
```

### Teardown

```bash
cd deploy/aws/terraform
terraform destroy
```

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| OOM during build | Hugepages reserving too much RAM | `setup-ec2.sh` handles this automatically. If manual build, release hugepages first: set `memory_mib: 0` in `/etc/nitro_enclaves/allocator.yaml` then `sudo systemctl restart nitro-enclaves-allocator` |
| `AES-GCM decryption failed` | Client using stale encryption key | Stale node entries in GlobalConfig. Remove old nodes with `title-cli remove-node` |
| Enclave won't start | Insufficient hugepages | Check `grep Hugetlb /proc/meminfo`. Ensure no other Enclave is running. |
| `wasm_trusted` check fails | On-chain WASM hash doesn't match Enclave binary | Re-run Step 4 (register WASM hashes) with the EC2-built binaries |
| ALT creation fails after restart | New signing key, old ALT owner | Expected on re-registration. Run `title-cli create-alt` manually. |

---

## What's Next

| Goal | Guide |
|------|-------|
| Understand the architecture | [docs/architecture.md](../../docs/architecture.md) |
| Run locally instead | [deploy/local/README.md](../local/README.md) |
| Build an app with the SDK | [sdk/ts/README.md](../../sdk/ts/README.md) |
| Query indexed cNFTs | [indexer/README.md](../../indexer/README.md) |
| Troubleshooting | [docs/troubleshooting.md](../../docs/troubleshooting.md) |
