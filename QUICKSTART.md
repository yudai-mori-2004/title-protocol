# Quick Start

This guide walks you through running Title Protocol locally and registering content on Solana devnet. Read through the full document before starting — the setup has two phases, and Phase 1 must be completed before Phase 2 will work.

---

## On-Chain Architecture

### GlobalConfig: The Root of Trust

Every Title Protocol network has exactly **one GlobalConfig** — a single on-chain account (PDA) that anchors the entire trust chain. Think of it like a DNS root zone or a CA root certificate: everything in the protocol traces its authority back to this account.

```
                    +----------------------------+
                    |      GlobalConfig PDA      |
                    |  seeds = ["global-config"]  |
                    |                            |
                    |  authority (DAO wallet)    |
                    |  core_collection_mint      |
                    |  ext_collection_mint       |
                    |  trusted_node_keys[]       |--+
                    |  trusted_tsa_keys[]        |  |
                    |  trusted_wasm_modules[]    |  |
                    |  resource_limits           |  |
                    +----------------------------+  |
                                                    |  references
                    +----------------------------+  |
                    |   TeeNodeAccount PDA       |<-+
                    |  seeds = ["tee-node",       |
                    |           signing_pubkey]   |
                    |                            |
                    |  signing_pubkey            |
                    |  encryption_pubkey         |
                    |  gateway_pubkey            |
                    |  gateway_endpoint          |
                    |  tee_type                  |
                    |  measurements[]            |
                    +----------------------------+
```

**GlobalConfig** stores:

| Field | Description |
|-------|-------------|
| `authority` | The DAO wallet that controls all configuration updates. Only this key can add nodes, modules, or change settings. |
| `core_collection_mint` | The MPL Core Collection for provenance-graph cNFTs (Layer 1). |
| `ext_collection_mint` | The MPL Core Collection for extension-attribute cNFTs (Layer 2). |
| `trusted_node_keys` | List of TEE node signing pubkeys authorized to operate on this network. |
| `trusted_tsa_keys` | Trusted TSA (Time Stamping Authority) certificate hashes for timestamp verification. |
| `trusted_wasm_modules` | Registered WASM extension modules (extension\_id + SHA-256 hash of the binary). |
| `resource_limits` | On-chain resource limit ceiling (file size, concurrency, timeouts). Gateway clamps its defaults to never exceed these values. |

**TeeNodeAccount** (one per TEE node) stores the node's full specification — its cryptographic keys, gateway endpoint, TEE platform type, and expected attestation measurements. This PDA is created by the TEE itself during registration, ensuring that the node's internal keys are cryptographically bound to the on-chain record.

### Permissionless Protocol, Canonical Trust Root

The Title Protocol program is permissionless — anyone can deploy their own instance and create their own GlobalConfig on any Solana network. For development and testing, **each developer deploys their own program and GlobalConfig on devnet**. This provides full isolation: your own authority key, your own node registrations, and no interference from other developers.

On mainnet, the protocol has **one canonical trust root**: the GlobalConfig controlled by the DAO multi-sig. Only cNFTs minted into the **official collections** designated by this canonical GlobalConfig are recognized as protocol-canonical content records. When a verifier checks whether content is registered, they look up this specific GlobalConfig — it is the protocol's sole trust assumption.

```
Devnet (development):
  → Each developer deploys their own program + GlobalConfig
  → Full authority control over your own nodes and collections
  → No risk of conflicting with other developers

Mainnet (production):
  → One canonical program + GlobalConfig controlled by DAO multi-sig
  → Register your TEE node under the DAO's authority
  → Your node mints cNFTs into the official collections
```

See [Mainnet](#mainnet) for the production trust model.

---

## Two-Phase Setup

```
Phase 1: Network Setup (first-time only)
  Build + deploy Anchor program
  title-cli init-global
    +-- Create authority keypair
    +-- Create MPL Core collections
    +-- Initialize GlobalConfig PDA
    +-- Register WASM modules
    +-- Set ResourceLimits
    +-- Output network.json

Phase 2: Node Deployment (every time)
  deploy/local/setup.sh   (local)
  deploy/aws/setup-ec2.sh (production)
    +-- Build WASM + binaries
    +-- Start TEE + Gateway + TempStorage
    +-- title-cli register-node  (TEE signs -> authority co-signs)
    +-- title-cli create-tree    (Merkle trees for cNFT minting)
```

`network.json` is the bridge between the two phases. Phase 1 creates it; Phase 2 reads it. Each developer runs Phase 1 once to create their own isolated devnet environment.

---

## Phase 1: Network Setup

> Every developer runs Phase 1 once to create their own isolated GlobalConfig on devnet. This creates `keys/authority.json` and `network.json`, which Phase 2 depends on.

### Phase 1 Prerequisites

- Everything from [Phase 2 Prerequisites](#phase-2-prerequisites) below
- `cargo-build-sbf` (installed with Solana CLI)
- ~5 SOL on devnet (program deploy costs ~2 SOL; use `solana airdrop` or [faucet.solana.com](https://faucet.solana.com))

### Step 1: Build and Deploy the Anchor Program

Each developer deploys their own program instance on devnet. This ensures complete isolation — your own GlobalConfig PDA, your own collections, your own authority.

```bash
# 1. Generate a new program keypair
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json --force
solana-keygen pubkey programs/title-config/target/deploy/title_config-keypair.json
# Note this Program ID — you'll need it below.

# 2. Update declare_id! to match your new Program ID in these files:
#    - programs/title-config/src/lib.rs         — declare_id!("...")
#    - Anchor.toml                              — [programs.localnet] and [programs.devnet]
#    - crates/cli/src/commands/init_global.rs   — DEFAULT_PROGRAM_ID
#    - crates/cli/src/anchor.rs                 — test program IDs
#    - crates/tee/src/endpoints/register_node.rs — test program IDs
#    - sdk/ts/src/chain.ts                      — TITLE_CONFIG_PROGRAM_ID

# 3. Build the program (with your updated Program ID)
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
cd ../..

# 4. Deploy (uses your Solana CLI default wallet as payer — needs ~5 SOL)
solana program deploy programs/title-config/target/deploy/title_config.so \
  --program-id programs/title-config/target/deploy/title_config-keypair.json \
  --url devnet
```

### Step 2: Build WASM Modules

```bash
for dir in wasm/*/; do
  (cd "$dir" && cargo build --target wasm32-unknown-unknown --release)
done
```

### Step 3: Build the CLI

```bash
cargo build --release -p title-cli
```

### Step 4: Initialize GlobalConfig

```bash
./target/release/title-cli init-global --cluster devnet
```

This is **idempotent** — safe to run multiple times. It will:

1. Load or create an authority keypair at `keys/authority.json`
2. Create two MPL Core Collections (Core + Extension) if not already present
3. Call `initialize` to create the GlobalConfig PDA (skipped if it already exists)
4. Register the 4 built-in WASM modules via `add_wasm_module` (upsert — updates hash if already registered)
5. Set default ResourceLimits on-chain via `set_resource_limits` (file size caps, timeouts, etc.)
6. Write `network.json` to the project root

Both `keys/authority.json` and `network.json` are gitignored — they are local to your environment. After completion, proceed to Phase 2.

---

## Phase 2: Node Deployment

> Requires `network.json` from [Phase 1](#phase-1-network-setup). If you already ran `setup.sh` successfully, skip to [Register Content](#register-content).

### Phase 2 Prerequisites

| Tool | Required | Notes |
|------|----------|-------|
| [Rust](https://rustup.rs/) + `wasm32-unknown-unknown` target | Yes | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | Yes | |
| [Docker](https://docs.docker.com/get-docker/) (with Compose V2) | Yes | Local: PostgreSQL for indexer. AWS: Gateway container |
| [Python 3](https://www.python.org/) | Yes | `setup.sh` uses it to parse `network.json` (pre-installed on macOS/most Linux) |
| [Node.js](https://nodejs.org/) 20+ | Optional | Local indexer (skipped if not installed). Not needed for the registration flow |
| ~0.6 SOL on devnet | Yes | `setup.sh` checks the balance and pauses if insufficient. Get SOL: `solana airdrop 2 --url devnet` or [faucet.solana.com](https://faucet.solana.com). See [Wallet Roles](#wallet-roles) |

### Wallet Roles

Title Protocol uses three distinct wallet types. All keypairs are managed in the `keys/` directory at the project root (see [`keys/README.md`](keys/README.md)):

| Wallet | File | Purpose | Lifecycle |
|--------|------|---------|-----------|
| **Authority** | `keys/authority.json` | Controls GlobalConfig PDA. Adds nodes, WASM modules, changes settings. | Created by `title-cli init-global` during Phase 1. Never committed to the repository. |
| **Operator** | `keys/operator.json` | Funds TEE internal wallet with SOL for TX fees and Merkle Tree rent. | Auto-created by `setup.sh` if missing. Every node operator needs this. |
| **TEE Internal** | *(in-memory only)* | Signs on-chain transactions (node registration, cNFT minting). Acts as payer. | Ephemeral — regenerated on every TEE restart. |

**SOL flow:**

```
keys/operator.json  --(fund_tee_wallet)-->  TEE Internal Wallet  --(on-chain TXs)-->  Solana
     (~0.6 SOL)                               (ephemeral)
```

The operator wallet is your personal funding source. It sends SOL to the TEE's ephemeral wallet, which then pays for all on-chain transactions (node registration, Merkle Tree creation, cNFT minting).

### Node Architecture

```
Client --> Gateway (:3000) --> TempStorage (:3001) --> TEE (:4000) --> Solana
                                                       |
                                                  WASM Modules
                                                  (phash, etc.)
```

- **Gateway** — Client-facing HTTP server. Handles uploads, relays requests to the TEE, and optionally broadcasts transactions.
- **TEE** — Trusted Execution Environment. Verifies C2PA signatures, runs WASM extensions, and signs transactions with ephemeral keys that exist only in enclave memory.
- **TempStorage** — Object storage for encrypted payloads (auto-deleted after processing).

> **Indexer** is a separate component (not part of the node). It indexes cNFTs from on-chain Merkle Trees into PostgreSQL for querying. See `indexer/` for details.

### Deploying Locally

```bash
# 1. Create .env (the default SOLANA_RPC_URL is already set — just copy)
cp .env.example .env

# 2. Start everything (builds, starts services, registers node, creates Merkle Trees)
#    Auto-creates keys/operator.json if missing. Pauses for SOL funding if needed.
./deploy/local/setup.sh
```

`setup.sh` handles the entire process:

| Step | What | Details |
|------|------|---------|
| 0 | Prerequisite check | Verifies Rust, Solana CLI, Docker, .env, network.json, SOL balance |
| 1 | Build WASM modules | 4 modules → `wasm-modules/` |
| 2 | Build host binaries | TEE, Gateway, TempStorage, CLI |
| 3 | Start TEE | MockRuntime, port 4000 |
| 4 | Start services | TempStorage (:3001), Gateway (:3000), PostgreSQL (:5432), Indexer (:5001, if Node.js available) |
| 5 | Register TEE node | On-chain node registration (auto-signs if authority keypair exists) |
| 6 | Create Merkle Trees | Core + Extension trees for cNFT minting |
| 7 | Health check | Verifies all services are responding |

```bash
# View logs
tail -f /tmp/title-tee.log
tail -f /tmp/title-gateway.log

# Stop everything
./deploy/local/teardown.sh
```

See [`deploy/local/README.md`](deploy/local/README.md) for details on individual process management.

### Vendor-Neutral Design

The protocol core is **vendor-neutral**; all vendor-specific code is isolated behind traits (`TeeRuntime`, `TempStorage`) and Cargo feature flags.

| Vendor | Path | TempStorage | TEE Platform | Feature Flag | Status |
|--------|------|-------------|-------------|--------------|--------|
| **Local** | `deploy/local/` | Local HTTP file server | MockRuntime | `vendor-local` | Available |
| AWS Nitro | `deploy/aws/` | S3-compatible | Nitro Enclaves | `vendor-aws` | Available |

> To add a new vendor implementation, implement the `TeeRuntime` and `TempStorage` traits, create a `deploy/<vendor>/` directory, and add a corresponding Cargo feature flag. See `deploy/aws/` for reference.

### Deploying with AWS (EC2 + Nitro Enclaves)

**Additional prerequisites:** AWS CLI configured (`aws configure`), Terraform 1.5+. The default instance type is `c5.xlarge` (~$0.10/hr).

Each EC2 instance runs one TEE node. Terraform supports deploying multiple nodes in parallel — increase `node_count` to scale out. Each node gets a dedicated Elastic IP, registers independently on-chain, and operates as a separate TEE.

```bash
# 1. Create an SSH key pair (skip if you already have one registered in AWS)
mkdir -p deploy/aws/keys
aws ec2 create-key-pair \
  --key-name title-protocol-devnet \
  --query 'KeyMaterial' \
  --output text > deploy/aws/keys/title-protocol-devnet.pem
chmod 400 deploy/aws/keys/title-protocol-devnet.pem

# 2. Provision AWS resources (EC2 + Elastic IP, S3, IAM, Security Group)
cd deploy/aws/terraform
terraform init && terraform apply                     # 1 node (default)
# terraform apply -var="node_count=3"                 # scale to 3 nodes
cd ../../..

# 3. Get node IPs and S3 credentials
cd deploy/aws/terraform
terraform output nodes                                # list all nodes with IPs
terraform output -raw s3_access_key_id                # S3 access key
terraform output -raw s3_secret_access_key            # S3 secret key
cd ../../..
```

**For each node**, SSH in, clone the repo, configure `.env`, and deploy:

```bash
# 4. SSH into the node (replace NODE_IP with the Elastic IP from terraform output)
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP

# --- on EC2 ---
git clone <REPO_URL> ~/title-protocol && cd ~/title-protocol
cp .env.example .env
# Edit .env — see the mapping table below for which values to set
```

**Terraform output → .env mapping:**

| `.env` variable | Terraform command | Notes |
|-----------------|-------------------|-------|
| `SOLANA_RPC_URL` | *(already set in .env.example)* | Change for dedicated RPC |
| `S3_ENDPOINT` | `terraform output -raw s3_bucket_endpoint` | e.g. `https://s3.ap-northeast-1.amazonaws.com` |
| `S3_BUCKET` | `terraform output -raw s3_bucket_name` | e.g. `title-uploads-devnet` |
| `S3_ACCESS_KEY` | `terraform output -raw s3_access_key_id` | |
| `S3_SECRET_KEY` | `terraform output -raw s3_secret_access_key` | |

```bash
# 5. Copy keypairs from local to EC2 (enables auto-signing during setup)
#    Both files are in keys/ locally (created by Phase 1 and setup.sh respectively).
#    operator.json is auto-created by setup-ec2.sh if missing.
scp -i deploy/aws/keys/title-protocol-devnet.pem \
  keys/authority.json keys/operator.json \
  ec2-user@NODE_IP:~/title-protocol/keys/

# 6. SSH back in and deploy everything
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP
cd ~/title-protocol
./deploy/aws/setup-ec2.sh
```

> **First run:** Builds WASM modules, Rust binaries, Docker images, and a Nitro Enclave EIF. Expect **20-40 minutes** on first run.

> **SOL funding:** `setup-ec2.sh` checks the operator wallet balance and pauses if insufficient. Devnet airdrops are often rate-limited from EC2 IPs — you can send SOL from your local wallet instead: `solana transfer <EC2_WALLET_PUBKEY> 2 --url devnet`.

`setup-ec2.sh` handles the entire process:

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
| 8 | Register TEE node | On-chain registration (auto-signs if authority keypair exists) |
| 9 | Create Merkle Trees | Core + Extension trees for cNFT minting |
| 10 | Health check | Verifies all services |

```bash
# View logs
docker compose -f deploy/aws/docker-compose.production.yml logs -f  # Gateway
sudo nitro-cli console --enclave-id $(nitro-cli describe-enclaves | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['EnclaveID'])")  # TEE (Enclave console)

# Stop everything
sudo nitro-cli terminate-enclave --all
docker compose -f deploy/aws/docker-compose.production.yml down
pkill title-proxy || true
```

> **Quick test** — verify a C2PA-signed photo through the Nitro Enclave:
>
> ```bash
> # From your local machine (replace NODE_IP with the Elastic IP):
> cd integration-tests && npm install
> npx tsx register-photo.ts NODE_IP ./fixtures/pixel_photo_ramen.jpg \
>   --wallet keys/operator.json --skip-sign
> ```
>
> You should see `tee_type: aws_nitro` in the output, confirming real Nitro Enclave verification.

See [`deploy/aws/README.md`](deploy/aws/README.md) for the full architecture diagram and troubleshooting.

### TEE Node Registration

The node registration uses a **partial-signature pattern**:

1. `title-cli register-node` calls TEE `/register-node`
2. TEE generates ephemeral signing/encryption keypairs
3. TEE builds a `register_tee_node` transaction, signs it as payer (proves key ownership)
4. TEE returns the partially-signed transaction

Then, depending on the environment:

| `keys/authority.json` | Behavior |
|---|---|
| Exists (your own GlobalConfig) | CLI loads authority, co-signs, broadcasts immediately |
| Does not exist (e.g. DAO-controlled mainnet) | CLI outputs partial TX for multi-sig approval |

On devnet, `keys/authority.json` is created during Phase 1 (`init-global`), so `setup.sh` auto-signs during development.

### Node Lifecycle

**Registration:** `title-cli register-node` + `title-cli create-tree`. The TEE signs transactions with its internal key, proving ownership. The authority co-signs to approve.

**Restart:** TEE nodes are stateless. On restart, all keys are regenerated. The node must re-register and create a new Merkle Tree. `setup.sh` handles this automatically.

**Decommission:** Remove the node's signing pubkey from GlobalConfig using the authority key. Existing cNFTs minted by the node remain valid.

---

## Register Content

With a running node, you can register C2PA-signed content on-chain. The flow uses end-to-end encryption — even the node operator cannot see the raw content.

### How It Works

```
1. Client                    2. Client                  3. Client
   |                            |                          |
   |  Read GlobalConfig         |  POST /upload-url        |  PUT <upload_url>
   |  (on-chain PDA)            |  -------------->         |  -------------->
   |  --> Solana RPC             |  <--------------         |  <--------------
   |  encryption_pubkey         |  upload_url              |  200 OK
   |                            |  download_url            |
   v                            v                          v

4. Client                    5. Client                  6. Client
   |                            |                          |
   |  POST /verify              |  Upload signed_json      |  POST /sign
   |  -------------->           |  to Arweave (via Irys)   |  -------------->
   |  Gateway -> TEE            |  -------------->         |  Gateway -> TEE
   |  <--------------           |  <--------------         |  <--------------
   |  encrypted results         |  ar://<tx_id>            |  partial_txs[]
   |                            |                          |
   v                            v                          v  broadcast
```

1. **Get node info** — Fetch the TEE's X25519 encryption pubkey from on-chain GlobalConfig + TeeNodeAccount PDA
2. **Get upload URL** — Request a presigned upload URL from the Gateway
3. **Upload encrypted payload** — Encrypt the content + owner wallet with ECDH (X25519 + HKDF-SHA256 + AES-256-GCM), upload to temp storage
4. **Verify** — The Gateway relays to the TEE, which decrypts, verifies C2PA signatures, builds the provenance graph, and runs WASM extensions. Results are returned encrypted
5. **Store results** — Upload the signed JSON to permanent storage (Arweave via Irys)
6. **Sign & Mint** — The TEE creates cNFT mint transactions. The client broadcasts them to Solana

### Using the TypeScript SDK

```bash
cd sdk/ts && npm install && npm run build
```

```typescript
import {
  TitleClient,
  fetchGlobalConfig,
  encryptPayload,
  decryptResponse,
} from "@title-protocol/sdk";
import { Connection } from "@solana/web3.js";

// 1. Fetch GlobalConfig from on-chain (reads GlobalConfig + all TeeNodeAccount PDAs)
const connection = new Connection("https://api.devnet.solana.com");
const globalConfig = await fetchGlobalConfig(connection);

// 2. Initialize client
const client = new TitleClient({
  teeNodes: globalConfig.trusted_tee_nodes.map(n => n.gateway_endpoint),
  solanaRpcUrl: "https://api.devnet.solana.com",
  globalConfig,
});

// 3. Select a node (health-checks each gateway, skips unreachable nodes)
const session = await client.selectNode();

// 4. Encrypt content with TEE's X25519 public key (E2EE)
const teePubkey = Buffer.from(session.encryptionPubkey, "base64");
const payload = JSON.stringify({
  owner_wallet: ownerWallet,
  content: contentBase64,
});
const { symmetricKey, encryptedPayload } = await encryptPayload(
  teePubkey,
  new TextEncoder().encode(payload),
);

// 5. Upload encrypted payload to temporary storage
const { downloadUrl } = await client.upload(session.gatewayUrl, encryptedPayload);

// 6. Verify (TEE decrypts, verifies C2PA, builds provenance graph)
const encrypted = await client.verify(session.gatewayUrl, {
  download_url: downloadUrl,
  processor_ids: ["core-c2pa"],
});
const resultBytes = await decryptResponse(
  symmetricKey, encrypted.nonce, encrypted.ciphertext,
);
const verifyResult = JSON.parse(new TextDecoder().decode(resultBytes));

// 7. Upload signed_json to Arweave, then sign + mint cNFT
const arweaveUri = await uploadToArweave(verifyResult);
const { partial_txs } = await client.sign(session.gatewayUrl, {
  recent_blockhash: blockhash,
  requests: [{ signed_json_uri: arweaveUri }],
});
// Co-sign partial_txs with user wallet and broadcast to Solana
```

### Using the Integration Tests as Reference

For a complete working example with real C2PA-signed test fixtures, see:

- `integration-tests/register-photo.ts` — End-to-end content registration
- `integration-tests/stress-test.ts` — Concurrent registration under load
- `integration-tests/fixtures/` — Sample C2PA-signed images

```bash
cd integration-tests
npm install

# Verify only (no Arweave upload, no cNFT minting)
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet keys/operator.json --skip-sign

# Full flow: verify + Arweave upload + cNFT mint + broadcast
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet keys/operator.json --broadcast
```

---

## Environment Variables

| Variable | Service | Description |
|----------|---------|-------------|
| `SOLANA_RPC_URL` | All | Solana RPC endpoint (**only required variable for local dev**) |
| `GATEWAY_SIGNING_KEY` | Gateway, CLI | Ed25519 secret key (64-char hex). Auto-generated by `setup.sh` if unset |
| `TEE_RUNTIME` | TEE | Runtime implementation (`mock`, `nitro`, etc.) |
| `PROXY_ADDR` | TEE | `direct` (direct HTTP) or `127.0.0.1:8000` (vsock bridge) |
| `CORE_COLLECTION_MINT` | TEE | Core Collection Mint address (auto-read from `network.json`) |
| `EXT_COLLECTION_MINT` | TEE | Extension Collection Mint address (auto-read from `network.json`) |
| `GATEWAY_PUBKEY` | TEE | Gateway's Ed25519 pubkey for request authentication |
| `GLOBAL_CONFIG_PDA` | Gateway | GlobalConfig PDA address. If set, Gateway fetches on-chain ResourceLimits at startup (auto-read from `network.json`) |
| `TEE_ENDPOINT` | Gateway | TEE server URL (e.g., `http://localhost:4000`) |
| `S3_ENDPOINT` | Gateway | S3-compatible storage endpoint (vendor-aws only) |
| `S3_ACCESS_KEY` | Gateway | Storage access key (vendor-aws only) |
| `S3_SECRET_KEY` | Gateway | Storage secret key (vendor-aws only) |
| `S3_BUCKET` | Gateway | Bucket name for temp uploads (vendor-aws only) |

See [`.env.example`](.env.example) for the full list.

---

## Troubleshooting

### Port already in use

```
Error: Address already in use (os error 48)
```

A previous session's process is still running. Stop everything and retry:

```bash
./deploy/local/teardown.sh
./deploy/local/setup.sh
```

If a process still clings to a port, kill it directly:

```bash
lsof -ti :3000 | xargs kill   # replace 3000 with the blocked port
```

### `setup.sh` fails at node registration or Merkle Tree creation

Both steps require devnet SOL in your operator wallet. Check your balance:

```bash
solana balance $(solana-keygen pubkey keys/operator.json) --url devnet
```

If insufficient, request more:

```bash
solana airdrop 2 $(solana-keygen pubkey keys/operator.json) --url devnet
```

Then re-run `./deploy/local/setup.sh` (it skips already-running services and retries the failed steps).

### AES-GCM decryption failure on `/verify`

```
ペイロードの復号に失敗: AES-GCM復号に失敗しました
```

The SDK encrypted the payload with a stale TEE node's key. TEE nodes regenerate keys on every restart, but old node entries remain on-chain. The SDK (`selectNode()`) deduplicates by gateway endpoint and uses the most recently registered entry. If you still see this error after updating the SDK, restart with a clean slate:

```bash
./deploy/local/teardown.sh
./deploy/local/setup.sh
```

### Docker / PostgreSQL won't start

Make sure Docker Desktop (or the Docker daemon) is running:

```bash
docker info
```

Port 5432 may conflict with a local PostgreSQL installation. Stop it or change the port in `deploy/local/docker-compose.yml`.

---

## Mainnet

Mainnet uses the exact same on-chain structure as devnet. The only difference is social: there is one canonical GlobalConfig controlled by the protocol DAO, and all production TEE nodes are registered under its authority.

| Item | Value |
|------|-------|
| Program ID | *Not yet deployed* |
| GlobalConfig PDA | *Derived from program ID at launch* |
| Authority | *DAO multi-sig (Squads Protocol)* |

### Why the DAO GlobalConfig Is the Single Source of Truth

The GlobalConfig designates which **cNFT collections** are official. Content registered through the protocol is minted as cNFTs into these collections. When a verifier (app, marketplace, browser extension) checks whether content is Title Protocol-registered, it reads the canonical GlobalConfig and checks the cNFT against the official collections. This is the protocol's sole trust assumption — if you trust the DAO's governance, you trust the content records.

Anyone can deploy their own Title Protocol program and GlobalConfig. Those instances are fully functional, but their cNFTs live in separate collections that canonical verifiers don't recognize. Think of it like running your own DNS root — it works, but nobody else resolves from it.

### Trust Model

- The DAO multi-sig controls the authority key (no single person can modify the GlobalConfig)
- TEE nodes must pass remote attestation — the on-chain `measurements` field ensures only verified enclave code is trusted
- WASM module hashes are pinned — only binaries matching the registered SHA-256 hash can execute
- Collection Authority delegation is explicit — only registered TEE nodes can mint cNFTs into the official collections
- All GlobalConfig changes are on-chain and publicly auditable — the DAO's track record is transparent
