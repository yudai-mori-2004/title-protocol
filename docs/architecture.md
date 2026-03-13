# Architecture

Title Protocol のオンチェーン構造、信頼モデル、ノードアーキテクチャの概念説明。

セットアップ手順は [QUICKSTART.md](../QUICKSTART.md) を参照。

---

## On-Chain Structure

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

---

## Permissionless Protocol, Canonical Trust Root

The Title Protocol program is permissionless — anyone can deploy their own instance and create their own GlobalConfig on any Solana network. For development and testing, **each developer deploys their own program and GlobalConfig on devnet**. This provides full isolation: your own authority key, your own node registrations, and no interference from other developers.

On mainnet, the protocol has **one canonical trust root**: the GlobalConfig controlled by the DAO multi-sig. Only cNFTs minted into the **official collections** designated by this canonical GlobalConfig are recognized as protocol-canonical content records.

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

## Wallet Roles

Title Protocol uses three distinct wallet types. All keypairs are managed in the `keys/` directory (see [`keys/README.md`](../keys/README.md)):

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

---

## Node Architecture

```
Client --> Gateway (:3000) --> TempStorage (:3001) --> TEE (:4000) --> Solana
                                                       |
                                                  WASM Modules
                                                  (phash, etc.)
```

- **Gateway** — Client-facing HTTP server. Handles uploads, relays requests to the TEE, and optionally broadcasts transactions.
- **TEE** — Trusted Execution Environment. Verifies C2PA signatures, runs WASM extensions, and signs transactions with ephemeral keys that exist only in enclave memory.
- **TempStorage** — Object storage for encrypted payloads (auto-deleted after processing).

> **Indexer** is a separate component (not part of the node). It indexes cNFTs from on-chain Merkle Trees into PostgreSQL for querying. See [`indexer/README.md`](../indexer/README.md) for details.

---

## Vendor-Neutral Design

The protocol core is **vendor-neutral**; all vendor-specific code is isolated behind traits (`TeeRuntime`, `TempStorage`) and Cargo feature flags.

| Vendor | Path | TempStorage | TEE Platform | Feature Flag | Status |
|--------|------|-------------|-------------|--------------|--------|
| **Local** | `deploy/local/` | Local HTTP file server | MockRuntime | `vendor-local` | Available |
| AWS Nitro | `deploy/aws/` | S3-compatible | Nitro Enclaves | `vendor-aws` | Available |

> To add a new vendor implementation, implement the `TeeRuntime` and `TempStorage` traits, create a `deploy/<vendor>/` directory, and add a corresponding Cargo feature flag. See `deploy/aws/` for reference.

---

## TEE Node Registration

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

---

## Node Lifecycle

**Registration:** `title-cli register-node` + `title-cli create-tree`. The TEE signs transactions with its internal key, proving ownership. The authority co-signs to approve.

**Restart:** TEE nodes are stateless. On restart, all keys are regenerated. The node must re-register and create a new Merkle Tree. `setup.sh` handles this automatically.

**Decommission:** Remove the node's signing pubkey from GlobalConfig using `title-cli remove-node`. Existing cNFTs minted by the node remain valid.

---

## Content Registration Flow

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

For SDK usage, see [`sdk/ts/README.md`](../sdk/ts/README.md).

---

## Mainnet Trust Model

Mainnet uses the exact same on-chain structure as devnet. The only difference is social: there is one canonical GlobalConfig controlled by the protocol DAO, and all production TEE nodes are registered under its authority.

| Item | Value |
|------|-------|
| Program ID | *Not yet deployed* |
| GlobalConfig PDA | *Derived from program ID at launch* |
| Authority | *DAO multi-sig (Squads Protocol)* |

### Why the DAO GlobalConfig Is the Single Source of Truth

The GlobalConfig designates which **cNFT collections** are official. Content registered through the protocol is minted as cNFTs into these collections. When a verifier (app, marketplace, browser extension) checks whether content is Title Protocol-registered, it reads the canonical GlobalConfig and checks the cNFT against the official collections. This is the protocol's sole trust assumption — if you trust the DAO's governance, you trust the content records.

Anyone can deploy their own Title Protocol program and GlobalConfig. Those instances are fully functional, but their cNFTs live in separate collections that canonical verifiers don't recognize. Think of it like running your own DNS root — it works, but nobody else resolves from it.

### Trust Chain

- The DAO multi-sig controls the authority key (no single person can modify the GlobalConfig)
- TEE nodes must pass remote attestation — the on-chain `measurements` field ensures only verified enclave code is trusted
- WASM module hashes are pinned — only binaries matching the registered SHA-256 hash can execute
- Collection Authority delegation is automatic — `register_tee_node` atomically adds the node's signing\_pubkey to both the GlobalConfig and the MPL Core UpdateDelegate plugin, ensuring only registered TEE nodes can mint cNFTs into the official collections
- All GlobalConfig changes are on-chain and publicly auditable — the DAO's track record is transparent
