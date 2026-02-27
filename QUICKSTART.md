# Quick Start

This guide walks you through the on-chain architecture of Title Protocol and how to deploy your own instance on Solana devnet.

## GlobalConfig: The Root of Trust

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

### Mainnet vs. Devnet

On **mainnet**, there will be one canonical GlobalConfig controlled by the protocol DAO. All production TEE nodes, WASM modules, and collections are registered under this single authority.

On **devnet**, you are free to deploy your own program and create your own GlobalConfig. This lets you:

- Experiment with the full protocol stack without needing permission
- Run your own TEE nodes against your own trust root
- Test WASM module registration and cNFT minting end-to-end
- Develop applications on top of the protocol in an isolated environment

The on-chain structure is identical in both cases — devnet is just mainnet without the social consensus on which GlobalConfig is canonical.

## Official Devnet Reference

The project maintains a reference GlobalConfig on devnet for integration testing:

| Item | Value |
|------|-------|
| Program ID | `GXo7dQ4kW8oeSSSK2Lhaw1jakNps1fSeUHEfeb7dRsYP` |
| GlobalConfig PDA | `JCY1KfHLVR1YNAUcDS3S2qSY7ofhTGz9WrqcHLiubs5S` |
| Authority | `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna` |
| Core Collection | `CGoxGQtbgNGJaegRzV6yGr8BFkHaphvsLjBYbFKPhWPm` |
| Extension Collection | `A8FFxPMh8vXM94pnqJ3fUuv9BPcmj9AMbMLVe8pWHvz1` |

```bash
solana account JCY1KfHLVR1YNAUcDS3S2qSY7ofhTGz9WrqcHLiubs5S --url devnet
```

## Two-Phase Setup

Title Protocol deployment is split into two independent phases:

```
Phase 1: Network Setup (once, from your local machine)
  title-cli init-global
    +-- Deploy Anchor program
    +-- Create MPL Core collections
    +-- Initialize GlobalConfig PDA
    +-- Register WASM modules
    +-- Output network.json

Phase 2: Node Deployment (per node, on EC2)
  deploy/aws/setup-ec2.sh
    +-- Build WASM + binaries + Enclave
    +-- Start TEE + Gateway + Indexer
    +-- title-cli register-node  (TEE signs -> authority co-signs)
    +-- title-cli create-tree    (Merkle trees for cNFT minting)
```

`network.json` is the bridge between the two phases. Phase 1 creates it; Phase 2 reads it.

---

## Phase 1: Network Setup (Local)

### Prerequisites

- [Rust](https://rustup.rs/) with `wasm32-unknown-unknown` target
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (v2.0+)
- `cargo-build-sbf` (installed with Solana CLI)
- ~5 SOL on devnet (program deploy costs ~2 SOL; use `solana airdrop` or [faucet.solana.com](https://faucet.solana.com))

### Step 1: Build and Deploy the Anchor Program

```bash
# Build
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

> **Custom Program ID:** The binary contains a hardcoded program ID (`declare_id!` in `lib.rs`). To deploy under a different ID, generate a new keypair with `solana-keygen new -o program-keypair.json`, update `declare_id!` to match, and rebuild.

```bash
# Deploy (uses the default program ID)
solana program deploy target/deploy/title_config.so \
  --url devnet \
  --keypair ~/.config/solana/id.json
```

### Step 2: Build WASM Modules

```bash
# From the project root
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

### What You Get

```
network.json
{
  "cluster": "devnet",
  "program_id": "GXo7dQ4kW8oeSSSK2Lhaw1jakNps1fSeUHEfeb7dRsYP",
  "global_config_pda": "JCY1KfHLVR1YNAUcDS3S2qSY7ofhTGz9WrqcHLiubs5S",
  "authority": "wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna",
  "core_collection_mint": "CGox...",
  "ext_collection_mint": "A8FF...",
  "wasm_modules": { "phash-v1": { "hash": "ab12..." }, ... }
}
```

Commit `network.json` to the repository. Every node reads it at startup.

```bash
solana account <global_config_pda> --url devnet
```

---

## Phase 2: Node Deployment (EC2)

### Node Architecture

```
Client --> Gateway (:3000) --> TEE --> Solana
               |                 |
               v                 v
          Temp Storage      WASM Modules
          (S3-compatible)   (phash, etc.)
```

- **Gateway** — Client-facing HTTP server. Handles uploads, relays requests to the TEE, and optionally broadcasts transactions.
- **TEE** — Trusted Execution Environment. Verifies C2PA signatures, runs WASM extensions, and signs transactions with ephemeral keys that exist only in enclave memory.
- **Temp Storage** — S3-compatible object storage for encrypted payloads (auto-deleted after processing).
- **Proxy** — Mediates TEE network access when the enclave has no direct network (e.g., vsock on Nitro).

### Vendor-Neutral Design

The protocol core is **vendor-neutral**; all vendor-specific code is isolated behind traits (`TeeRuntime`, `TempStorage`) and Cargo feature flags.

| Example | Path | TEE Platform | Status |
|---------|------|-------------|--------|
| AWS Nitro | `deploy/aws/` | AWS Nitro Enclaves | Available |

> To add a new vendor implementation, implement the `TeeRuntime` and `TempStorage` traits, create a `deploy/<vendor>/` directory, and add a corresponding Cargo feature flag. See `deploy/aws/` for reference.

### Deploying with the AWS Example

```bash
# 1. Provision AWS resources (EC2, S3, IAM, Security Group)
cd deploy/aws/terraform
terraform init && terraform apply

# 2. SSH into the EC2 instance
ssh -i deploy/aws/keys/<key>.pem ec2-user@<IP>

# 3. Clone and configure
git clone <REPO_URL> ~/title-protocol
cd ~/title-protocol
cp .env.example .env
# Edit .env with Terraform output values (S3 keys, RPC URL, etc.)

# 4. Deploy everything (builds, enclave, services, registration)
./deploy/aws/setup-ec2.sh
```

### What setup-ec2.sh Does

| Step | What | Tool |
|------|------|------|
| 0 | Load `.env` + `network.json`, validate | Shell |
| 1 | Build 4 WASM modules | `cargo build --target wasm32-unknown-unknown` |
| 2 | Build host binaries (`title-cli`, `title-proxy`) | `cargo build --release` |
| 3 | Build Enclave Image File (EIF) | `nitro-cli build-enclave` |
| 4 | Start TEE (Nitro Enclave or MockRuntime) | `nitro-cli run-enclave` |
| 5 | Start Proxy (Enclave mode only) | `title-proxy` |
| 6 | Start Gateway + Indexer + PostgreSQL | `docker compose` |
| 7 | Verify S3 access | `aws s3` |
| 8 | Register TEE node on-chain | `title-cli register-node` |
| 9 | Create Merkle Trees | `title-cli create-tree` |
| 10 | Health checks (TEE, Gateway, Indexer, RPC) | `curl` |

Total setup time: ~15-30 minutes (first build). See [`deploy/aws/README.md`](deploy/aws/README.md) for full details.

### TEE Node Registration (Step 8)

The node registration uses a **partial-signature pattern**:

1. `title-cli register-node` calls TEE `/register-node`
2. TEE generates ephemeral signing/encryption keypairs
3. TEE builds a `register_tee_node` transaction, signs it as payer (proves key ownership)
4. TEE returns the partially-signed transaction

Then, depending on the environment:

| Environment | `programs/title-config/keys/authority.json` | Behavior |
|---|---|---|
| **Devnet** | Exists | CLI loads authority, co-signs, broadcasts immediately |
| **Mainnet** | Does not exist | CLI outputs partial TX for DAO multi-sig approval |

After registration, `GlobalConfig.trusted_node_keys` gains the new node's pubkey, and a `TeeNodeAccount` PDA is created with the node's full specification.

### Node Lifecycle

**Registration:** `title-cli register-node` + `title-cli create-tree`. The TEE signs transactions with its internal key, proving ownership. The authority co-signs to approve.

**Restart:** TEE nodes are stateless. On restart, all keys are regenerated. The node must re-register and create a new Merkle Tree. `setup-ec2.sh` handles this automatically.

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
2. **Get upload URL** — Request a presigned S3 upload URL from the Gateway
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

// 3. Select a node (sync — resolved from on-chain GlobalConfig)
const session = client.selectNode();

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
npx tsx register-photo.ts
```

---

## Environment Variables

| Variable | Service | Description |
|----------|---------|-------------|
| `TEE_RUNTIME` | TEE | Runtime implementation (`mock`, `nitro`, etc.) |
| `PROXY_ADDR` | TEE | `direct` (direct HTTP) or `127.0.0.1:8000` (vsock bridge) |
| `CORE_COLLECTION_MINT` | TEE | Core Collection Mint address |
| `EXT_COLLECTION_MINT` | TEE | Extension Collection Mint address |
| `GATEWAY_PUBKEY` | TEE | Gateway's Ed25519 pubkey for request authentication |
| `GATEWAY_SIGNING_KEY` | Gateway | Gateway's Ed25519 secret key (hex) |
| `TEE_ENDPOINT` | Gateway | TEE server URL (e.g., `http://localhost:4000`) |
| `S3_ENDPOINT` | Gateway | S3-compatible storage endpoint |
| `S3_ACCESS_KEY` | Gateway | Storage access key |
| `S3_SECRET_KEY` | Gateway | Storage secret key |
| `S3_BUCKET` | Gateway | Bucket name for temp uploads |
| `SOLANA_RPC_URL` | All | Solana RPC endpoint |

See [`.env.example`](.env.example) for the full list.

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

The only behavioral difference between devnet and mainnet is in `title-cli register-node`:
- **Devnet:** If `programs/title-config/keys/authority.json` exists locally, the CLI auto-signs and broadcasts
- **Mainnet:** The CLI outputs a partial transaction for the DAO to review and co-sign
