# Quick Start

This guide walks you through the on-chain architecture of Title Protocol and how to deploy your own instance on Solana devnet.

## GlobalConfig: The Root of Trust

Every Title Protocol network has exactly **one GlobalConfig** — a single on-chain account (PDA) that anchors the entire trust chain. Think of it like a DNS root zone or a CA root certificate: everything in the protocol traces its authority back to this account.

```
                    ┌──────────────────────────┐
                    │      GlobalConfig PDA     │
                    │  seeds = ["global-config"] │
                    │                          │
                    │  authority (DAO wallet)   │
                    │  core_collection_mint     │
                    │  ext_collection_mint      │
                    │  trusted_node_keys[]      │──┐
                    │  trusted_tsa_keys[]       │  │
                    │  trusted_wasm_modules[]   │  │
                    └──────────────────────────┘  │
                                                  │  references
                    ┌──────────────────────────┐  │
                    │   TeeNodeAccount PDA     │◄─┘
                    │  seeds = ["tee-node",     │
                    │           signing_pubkey] │
                    │                          │
                    │  signing_pubkey           │
                    │  encryption_pubkey        │
                    │  gateway_pubkey           │
                    │  gateway_endpoint         │
                    │  tee_type                 │
                    │  measurements[]           │
                    └──────────────────────────┘
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

You can inspect these accounts with the Solana CLI:

```bash
solana account JCY1KfHLVR1YNAUcDS3S2qSY7ofhTGz9WrqcHLiubs5S --url devnet
```

## Quick Start: Deploy Your Own GlobalConfig on Devnet

### Prerequisites

- [Rust](https://rustup.rs/) with `wasm32-unknown-unknown` target
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (v2.0+)
- `cargo-build-sbf` (installed with Solana CLI)
- [Node.js](https://nodejs.org/) 22+
- ~5 SOL on devnet (program deploy costs ~2 SOL; use `solana airdrop` or [faucet.solana.com](https://faucet.solana.com))

### Step 1: Build the Anchor Program

```bash
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

This produces `target/deploy/title_config.so`.

### Step 2: Deploy to Devnet

```bash
# Generate a keypair for your authority (or use an existing one)
solana-keygen new -o my-authority.json

# Fund it
solana airdrop 2 $(solana-keygen pubkey my-authority.json) --url devnet
# If airdrop fails due to rate limits, use https://faucet.solana.com

# Generate a program keypair (determines the on-chain address)
solana-keygen new -o program-keypair.json
solana-keygen pubkey program-keypair.json   # Note this Program ID
```

> **Important:** The binary contains a hardcoded program ID (`declare_id!` in `lib.rs`). Before deploying, update it to match your program keypair, then **rebuild Step 1**:
>
> ```bash
> # Edit programs/title-config/src/lib.rs — replace the declare_id! value
> # with the pubkey from program-keypair.json, then rebuild:
> cd programs/title-config
> rm -f Cargo.lock && cargo generate-lockfile
> cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
> ```

```bash
# Deploy with the matching program keypair
solana program deploy target/deploy/title_config.so \
  --url devnet \
  --keypair my-authority.json \
  --program-id program-keypair.json
```

### Step 3: Build WASM Modules

```bash
# From the project root
for dir in wasm/*/; do
  (cd "$dir" && cargo build --target wasm32-unknown-unknown --release)
done
```

### Step 4: Initialize GlobalConfig

```bash
cd scripts && npm install

# Set your program ID (from Step 2)
export TITLE_CONFIG_PROGRAM_ID=<YOUR_PROGRAM_ID>

# Run the initialization script
node init-devnet.mjs \
  --rpc https://api.devnet.solana.com \
  --skip-tree \
  --skip-delegate
```

This will:
1. Load your authority keypair from `deploy/aws/keys/devnet-authority.json` (or generate one)
2. Create two MPL Core Collections (Core + Extension)
3. Call `initialize` to create the GlobalConfig PDA with your authority and collections
4. Register the 4 built-in WASM modules (phash-v1, hardware-google, c2pa-training-v1, c2pa-license-v1)

> **Tip:** To use a custom authority path, place your keypair JSON at `deploy/aws/keys/devnet-authority.json` before running the script.

### What You Get

After initialization, you have:

- **A GlobalConfig PDA** — deterministically derived from `seeds = ["global-config"]` + your program ID
- **Two MPL Core Collections** — ready to receive cNFTs minted by your TEE nodes
- **Registered WASM modules** — the protocol knows which extension binaries are trusted

You can verify the on-chain state:

```bash
# Show your GlobalConfig PDA address
node -e "
const { PublicKey } = require('@solana/web3.js');
const programId = new PublicKey('<YOUR_PROGRAM_ID>');
const [pda] = PublicKey.findProgramAddressSync(
  [Buffer.from('global-config')],
  programId
);
console.log('GlobalConfig PDA:', pda.toBase58());
"

# Inspect the account
solana account <PDA_ADDRESS> --url devnet
```

## Running a Node

Title Protocol nodes require real infrastructure — TEE hardware, S3-compatible storage, and network access to Solana RPC and Arweave. The protocol core is **vendor-neutral**; all vendor-specific code is isolated behind traits (`TeeRuntime`, `TempStorage`) and Cargo feature flags.

The repository provides example deployments in `deploy/`. You can use an existing example or create your own vendor implementation.

| Example | Path | TEE Platform | Status |
|---------|------|-------------|--------|
| AWS Nitro | `deploy/aws/` | AWS Nitro Enclaves | Available |

> To add a new vendor implementation, implement the `TeeRuntime` and `TempStorage` traits, create a `deploy/<vendor>/` directory, and add a corresponding Cargo feature flag. See `deploy/aws/` for reference.

### Node Architecture

```
Client ──► Gateway (:3000) ──► TEE ──► Solana
               │                 │
               ▼                 ▼
          Temp Storage      WASM Modules
          (S3-compatible)   (phash, etc.)
```

- **Gateway** — Client-facing HTTP server. Handles uploads, relays requests to the TEE, and optionally broadcasts transactions.
- **TEE** — Trusted Execution Environment. Verifies C2PA signatures, runs WASM extensions, and signs transactions with ephemeral keys that exist only in enclave memory.
- **Temp Storage** — S3-compatible object storage for encrypted payloads (auto-deleted after processing).
- **Proxy** — Mediates TEE network access when the enclave has no direct network (e.g., vsock on Nitro).

### Deploying with the AWS Example

The AWS example uses Terraform for infrastructure and a single setup script for deployment:

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

# 4. Deploy everything (builds, enclave, services)
./deploy/aws/setup-ec2.sh
```

`setup-ec2.sh` handles:
1. Building WASM modules and host binaries
2. Building the TEE Docker image and converting to an Enclave Image File (EIF)
3. Starting the Nitro Enclave with vsock bridge
4. Starting the HTTP Proxy (TEE <-> external network)
5. Starting Gateway, Indexer, and PostgreSQL via Docker Compose
6. Running `init-devnet.mjs` to register the node on-chain
7. Health checks on all services

Total setup time: ~15-30 minutes (first build). See [`deploy/aws/README.md`](deploy/aws/README.md) for full details.

### TEE Node Registration + Merkle Tree Creation

Once a node is running, it must be registered on-chain and a Merkle Tree must be created for cNFT minting. The setup script handles this automatically via `init-devnet.mjs`, but the process can also be run manually:

```bash
cd scripts

export TITLE_CONFIG_PROGRAM_ID=<YOUR_PROGRAM_ID>

node init-devnet.mjs \
  --rpc https://api.devnet.solana.com \
  --gateway http://<YOUR_NODE_IP>:3000
```

This performs:

1. **Funds the TEE wallet** — transfers SOL from the authority to the TEE's signing key for rent and transaction fees
2. **Calls TEE `/register-node`** — the TEE builds a `register_tee_node` transaction, signs it with its internal key (proving key ownership), and returns a partially-signed transaction
3. **Authority co-signs** — the script adds the authority signature and broadcasts
4. **Delegates Collection Authority** — grants the TEE permission to mint into your collections
5. **Calls TEE `/create-tree`** — the TEE builds a Merkle Tree creation transaction (depth=14, buffer=64, ~131KB), signs it, and the script broadcasts it

After this, the node is fully operational and can verify content and mint cNFTs.

### Node Lifecycle

**Registration:** When the node starts, `init-devnet.mjs` calls TEE `/register-node`. The TEE signs the transaction with its internal key, proving ownership of the signing/encryption keys. The authority co-signs to approve.

**Restart:** TEE nodes are stateless. On restart, all keys are regenerated. The node must re-register and create a new Merkle Tree. The deployment script handles this automatically.

**Decommission:** Remove the node's signing pubkey from GlobalConfig using the authority key. Existing cNFTs minted by the node remain valid.

## Register Content

With a running node, you can register C2PA-signed content on-chain. The flow uses end-to-end encryption — even the node operator cannot see the raw content.

### How It Works

```
1. Client                    2. Client                  3. Client
   │                            │                          │
   │  Read GlobalConfig         │  POST /upload-url        │  PUT <upload_url>
   │  (on-chain PDA)            │  ─────────────►          │  ─────────────►
   │  ──► Solana RPC            │  ◄─────────────          │  ◄─────────────
   │  encryption_pubkey         │  upload_url              │  200 OK
   │                            │  download_url            │
   │                            │                          │
   ▼                            ▼                          ▼

4. Client                    5. Client                  6. Client
   │                            │                          │
   │  POST /verify              │  Upload signed_json      │  POST /sign
   │  ─────────────►            │  to Arweave (via Irys)   │  ─────────────►
   │  Gateway → TEE             │  ─────────────►          │  Gateway → TEE
   │  ◄─────────────            │  ◄─────────────          │  ◄─────────────
   │  encrypted results         │  ar://<tx_id>            │  partial_txs[]
   │                            │                          │
   ▼                            ▼                          ▼  broadcast
```

1. **Get node info** — Look up the TEE's X25519 encryption pubkey from on-chain GlobalConfig + TeeNodeAccount PDA
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
import { TitleClient } from "@title-protocol/sdk";

// Point to your running node's Gateway
const client = new TitleClient({
  teeNodes: ["http://<YOUR_NODE_IP>:3000"],
  solanaRpcUrl: "https://api.devnet.solana.com",
  globalConfig: { /* fetched from on-chain GlobalConfig PDA */ },
});

// Step 1: Select a node (sync — from on-chain GlobalConfig)
const session = client.selectNode();

// Step 2-3: Encrypt and upload content
const { symmetricKey, downloadUrl } = await client.uploadEncrypted(
  contentBytes,     // Uint8Array — C2PA-signed image/video
  ownerWallet,      // string — Solana wallet to attribute
  session,
);

// Step 4: Verify (TEE processes the content)
const verifyResult = await client.verify(downloadUrl, symmetricKey);
// verifyResult contains: provenance graph, content hash, extension results

// Step 5: Upload signed_json to Arweave
const arweaveUri = await uploadToArweave(verifyResult.signedJson);

// Step 6: Sign and mint cNFT
const { partialTxs } = await client.sign(arweaveUri);
// Broadcast the transactions to Solana
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

## Environment Variable Reference

| Variable | Service | Description |
|----------|---------|-------------|
| `TEE_RUNTIME` | TEE | TEE runtime implementation (`mock`, `nitro`, etc.) |
| `PROXY_ADDR` | TEE | `direct` (direct HTTP) or `127.0.0.1:8000` (vsock bridge) |
| `CORE_COLLECTION_MINT` | TEE | Core Collection Mint address for cNFT minting |
| `EXT_COLLECTION_MINT` | TEE | Extension Collection Mint address for cNFT minting |
| `GATEWAY_PUBKEY` | TEE | Gateway's Ed25519 pubkey for request authentication |
| `TRUSTED_EXTENSIONS` | TEE | Comma-separated WASM extension IDs |
| `WASM_DIR` | TEE | Path to WASM binary directory |
| `TEE_ENDPOINT` | Gateway | TEE server URL |
| `S3_ENDPOINT` | Gateway | S3-compatible storage endpoint |
| `S3_ACCESS_KEY` | Gateway | Storage access key |
| `S3_SECRET_KEY` | Gateway | Storage secret key |
| `S3_BUCKET` | Gateway | Bucket name for temp uploads |
| `SOLANA_RPC_URL` | Gateway | Solana RPC endpoint |
| `TITLE_CONFIG_PROGRAM_ID` | Scripts | Override the default program ID |

See [`.env.example`](.env.example) for the full list.

## Mainnet

Mainnet uses the exact same on-chain structure as devnet. The only difference is social: there is one canonical GlobalConfig controlled by the protocol DAO, and all production TEE nodes are registered under its authority.

| Item | Value |
|------|-------|
| Program ID | *Not yet deployed* |
| GlobalConfig PDA | *Derived from program ID at launch* |
| Authority | *DAO multi-sig (Squads Protocol)* |

Once mainnet is live, the table above will be updated with the canonical addresses. The SDK and indexer will use these values by default.

The trust model on mainnet:
- The DAO multi-sig controls the authority key (no single person can modify the GlobalConfig)
- TEE nodes must pass remote attestation — the on-chain `measurements` field ensures only verified enclave code is trusted
- WASM module hashes are pinned — only binaries matching the registered SHA-256 hash can execute
- Collection Authority delegation is explicit — only registered TEE nodes can mint cNFTs into the official collections
