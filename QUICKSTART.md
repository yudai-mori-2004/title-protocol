# Quick Start

This guide walks you through running Title Protocol locally and registering content on Solana devnet.

## TL;DR — Fastest Path

The repository already includes a deployed devnet environment (`network.json`). To start a local node against it:

```bash
# 1. Create .env (the default SOLANA_RPC_URL is already set — just copy)
cp .env.example .env

# 2. Make sure you have devnet SOL (~0.6 SOL required)
solana airdrop 2 --url devnet    # skip if you already have SOL

# 3. Start all services
./deploy/local/setup.sh

# 4. Verify all services are running:
curl http://localhost:4000/health   # TEE
curl http://localhost:3000/health   # Gateway
```

> **First run:** The build step compiles 4 WASM modules and 4 Rust release binaries. Expect **10-20 minutes** on the first run; subsequent runs use the Cargo cache and finish in seconds.

This starts TEE, Gateway, TempStorage, Indexer, and PostgreSQL on your machine, registers the node on devnet, and creates Merkle Trees — all automatically.

**Ports used:** 3000 (Gateway), 3001 (TempStorage), 4000 (TEE), 5001 (Indexer), 5432 (PostgreSQL). If any of these are already in use, `setup.sh` will detect the existing process and skip starting a new one. To force a clean restart, run `./deploy/local/teardown.sh` first.

> **Quick test** — verify a C2PA-signed photo through the local TEE:
>
> ```bash
> cd integration-tests && npm install
> npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
>   --wallet ~/.config/solana/id.json --skip-sign
> ```
>
> You should see `STEP 4 /verify 完了` and a provenance graph output. This confirms the full pipeline (upload → encrypt → TEE verify → C2PA check) is working.
>
> **Stop everything:** `./deploy/local/teardown.sh`
>
> The rest of this document explains the on-chain architecture, how to deploy your own network from scratch, and how to register content with the full SDK. **Skip ahead to [Register Content](#register-content)** if you just want to integrate.

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

**TeeNodeAccount** (one per TEE node) stores the node's full specification — its cryptographic keys, gateway endpoint, TEE platform type, and expected attestation measurements. This PDA is created by the TEE itself during registration, ensuring that the node's internal keys are cryptographically bound to the on-chain record.

### Devnet Reference Environment

The repository ships with a pre-deployed devnet environment in `network.json`:

| Item | Value |
|------|-------|
| Program ID | `CD3KZe1NWppgkYSPJTq9g2JVYFBnm6ysGD1af8vJQMJq` |
| GlobalConfig PDA | `CLizWsiGX2Lva42boGuGuutessekt2HV8JyAHWYcmFYk` |
| Authority | `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna` |
| Core Collection | `H51zy5FPdoePeV4CHgB724SiuoUMfaRnFgYtxCTni9xv` |
| Extension Collection | `5cJGwZXp3YRM22hqHRPYNTfA528rfMv9TNZL9mZJLXFY` |

```bash
solana account CLizWsiGX2Lva42boGuGuutessekt2HV8JyAHWYcmFYk --url devnet
```

Most developers should use this existing environment. If you need your own isolated GlobalConfig (e.g., for testing program changes), see [Phase 1: Network Setup](#phase-1-network-setup-optional) below.

---

## Two-Phase Setup

```
Phase 1: Network Setup (optional — already done for devnet)
  title-cli init-global
    +-- Deploy Anchor program
    +-- Create MPL Core collections
    +-- Initialize GlobalConfig PDA
    +-- Register WASM modules
    +-- Output network.json

Phase 2: Node Deployment (required)
  deploy/local/setup.sh   (local)
  deploy/aws/setup-ec2.sh (production)
    +-- Build WASM + binaries
    +-- Start TEE + Gateway + TempStorage + Indexer
    +-- title-cli register-node  (TEE signs -> authority co-signs)
    +-- title-cli create-tree    (Merkle trees for cNFT minting)
```

`network.json` is the bridge between the two phases. Phase 1 creates it; Phase 2 reads it. **The repository already includes a `network.json` for the devnet reference environment, so you can go straight to Phase 2.**

---

## Phase 2: Node Deployment

> The TL;DR section above is a shortcut for this phase. If you already ran `setup.sh` successfully, skip to [Register Content](#register-content).

### Prerequisites

| Tool | Required | Notes |
|------|----------|-------|
| [Rust](https://rustup.rs/) + `wasm32-unknown-unknown` target | Yes | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | Yes | |
| [Docker](https://docs.docker.com/get-docker/) (with Compose V2) | Yes | PostgreSQL for indexer (uses port 5432) |
| [Python 3](https://www.python.org/) | Yes | `setup.sh` uses it to parse `network.json` (pre-installed on macOS/most Linux) |
| [Node.js](https://nodejs.org/) 20+ | Optional | Indexer (skipped if not installed) |
| ~0.6 SOL on devnet | Yes | `setup.sh` checks the balance and pauses if insufficient. Get SOL: `solana airdrop 2 --url devnet` or [faucet.solana.com](https://faucet.solana.com). Wallet is auto-created at `~/.config/solana/id.json` if missing |

### Node Architecture

```
Client --> Gateway (:3000) --> TempStorage (:3001) --> TEE (:4000) --> Solana
                                                       |
                                                  WASM Modules
                                                  (phash, etc.)

PostgreSQL (:5432) <-- Indexer (:5001)
```

- **Gateway** — Client-facing HTTP server. Handles uploads, relays requests to the TEE, and optionally broadcasts transactions.
- **TEE** — Trusted Execution Environment. Verifies C2PA signatures, runs WASM extensions, and signs transactions with ephemeral keys that exist only in enclave memory.
- **TempStorage** — Object storage for encrypted payloads (auto-deleted after processing).
- **Indexer** — Indexes cNFTs from on-chain Merkle Trees into PostgreSQL for querying.

### Deploying Locally

```bash
# 1. Create .env (the default SOLANA_RPC_URL is already set — just copy)
cp .env.example .env

# 2. Make sure you have devnet SOL (setup.sh will check and pause if insufficient)
solana airdrop 2 --url devnet

# 3. Start everything (builds, starts services, registers node, creates Merkle Trees)
./deploy/local/setup.sh
```

`setup.sh` handles the entire process:

| Step | What | Details |
|------|------|---------|
| 0 | Prerequisite check | Verifies Rust, Solana CLI, Docker, .env, network.json |
| 1 | Build WASM modules | 4 modules → `wasm-modules/` |
| 2 | Build host binaries | TEE, Gateway, TempStorage, CLI |
| 3 | Start TEE | MockRuntime, port 4000 |
| 4 | Start services | TempStorage (:3001), Gateway (:3000), PostgreSQL (:5432), Indexer (:5001) |
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

```bash
# 1. Provision AWS resources (EC2, S3, IAM, Security Group)
cd deploy/aws/terraform
terraform init && terraform apply

# 2. SSH into the EC2 instance, clone and configure
ssh -i deploy/aws/keys/<key>.pem ec2-user@<IP>
git clone <REPO_URL> ~/title-protocol
cd ~/title-protocol
cp .env.example .env
# Edit .env with Terraform output values (S3 keys, RPC URL, etc.)
exit

# 3. Copy the authority keypair from local (enables auto-signing during setup)
scp -i deploy/aws/keys/<key>.pem \
  programs/title-config/keys/authority.json \
  ec2-user@<IP>:~/title-protocol/programs/title-config/keys/

# 4. SSH back in and deploy everything
ssh -i deploy/aws/keys/<key>.pem ec2-user@<IP>
cd ~/title-protocol
./deploy/aws/setup-ec2.sh
```

### TEE Node Registration

The node registration uses a **partial-signature pattern**:

1. `title-cli register-node` calls TEE `/register-node`
2. TEE generates ephemeral signing/encryption keypairs
3. TEE builds a `register_tee_node` transaction, signs it as payer (proves key ownership)
4. TEE returns the partially-signed transaction

Then, depending on the environment:

| `programs/title-config/keys/authority.json` | Behavior |
|---|---|
| Exists (your own GlobalConfig) | CLI loads authority, co-signs, broadcasts immediately |
| Does not exist (e.g. DAO-controlled) | CLI outputs partial TX for multi-sig approval |

The repository includes the authority keypair for the devnet reference environment, so `setup.sh` auto-signs during local development.

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
  --wallet ~/.config/solana/id.json --skip-sign

# Full flow: verify + Arweave upload + cNFT mint + broadcast
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ~/.config/solana/id.json --broadcast
```

---

## Phase 1: Network Setup (Optional)

> **Skip this section if you're using the existing devnet reference environment** (i.e., the `network.json` already in the repository). Phase 1 is only needed if you want to deploy your own program and create your own GlobalConfig from scratch.

### Prerequisites

- Everything from [Phase 2 Prerequisites](#prerequisites) above
- `cargo-build-sbf` (installed with Solana CLI)
- ~5 SOL on devnet (program deploy costs ~2 SOL; use `solana airdrop` or [faucet.solana.com](https://faucet.solana.com))

### Step 1: Build and Deploy the Anchor Program

```bash
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

The repository includes a program keypair at `target/deploy/title_config-keypair.json` that matches the devnet reference Program ID (`CD3KZe1...`). For a fresh deployment with a new Program ID:

1. Generate a new keypair: `solana-keygen new -o target/deploy/title_config-keypair.json --force`
2. Get the new Program ID: `solana-keygen pubkey target/deploy/title_config-keypair.json`
3. Update `declare_id!` in the following files, then rebuild:
   - `programs/title-config/src/lib.rs` — `declare_id!("...")`
   - `Anchor.toml` — `[programs.localnet]` and `[programs.devnet]`
   - `crates/cli/src/commands/init_global.rs` — `DEFAULT_PROGRAM_ID`
   - `crates/cli/src/anchor.rs` — test program IDs
   - `crates/tee/src/endpoints/register_node.rs` — test program IDs
   - `sdk/ts/src/chain.ts` — `TITLE_CONFIG_PROGRAM_ID`

```bash
solana program deploy target/deploy/title_config.so \
  --url devnet \
  --keypair ~/.config/solana/id.json
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

1. Load or create an authority keypair at `programs/title-config/keys/authority.json`
2. Create two MPL Core Collections (Core + Extension) if not already present
3. Call `initialize` to create the GlobalConfig PDA (skipped if it already exists)
4. Register the 4 built-in WASM modules via `add_wasm_module` (upsert — updates hash if already registered)
5. Write `network.json` to the project root

After completion, commit the new `network.json` and proceed to Phase 2.

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

Both steps require devnet SOL. Check your balance:

```bash
solana balance --url devnet
```

If insufficient, request more:

```bash
solana airdrop 2 --url devnet
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

The trust model on mainnet:
- The DAO multi-sig controls the authority key (no single person can modify the GlobalConfig)
- TEE nodes must pass remote attestation — the on-chain `measurements` field ensures only verified enclave code is trusted
- WASM module hashes are pinned — only binaries matching the registered SHA-256 hash can execute
- Collection Authority delegation is explicit — only registered TEE nodes can mint cNFTs into the official collections
