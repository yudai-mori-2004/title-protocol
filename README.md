# Title Protocol

**The Identity Layer for Digital Content**

[C2PA](https://c2pa.org/) standardized how to **verify** digital content — who created it, with what tool, and that it hasn't been tampered with. But C2PA did not standardize how to **record** verification results. Each verification is performed locally and the result is consumed on the spot. Even if a service stores the result in its own database, that record can be altered or fabricated by the operator, and no third party can independently confirm it was done correctly.

Title Protocol closes this gap by making both the verification and the record trustless. A TEE (Trusted Execution Environment) performs C2PA verification in hardware-isolated memory — the node operator cannot see or tamper with the process. The TEE signs the results with its enclave-bound key, and the signed record is stored permanently on the Solana blockchain as a cNFT. Anyone can follow the on-chain trust chain to independently confirm that a given record was produced by an authorized TEE.

## How It Works

The client encrypts content and the destination wallet address with the TEE's public key (ECDH + AES-GCM), uploads the encrypted payload to temporary storage, and sends the URL to the Gateway. The Gateway relays to the TEE, which fetches and decrypts the payload, verifies C2PA signatures, and returns the results encrypted back to the client. The node operator cannot see the content or the destination wallet at any point.

```
Client (SDK)       Temp Storage       Gateway            TEE                 Solana
     |                  |                |                 |                    |
     |  encrypt payload |                |                 |                    |
     |  (ECDH+AES-GCM) |                |                 |                    |
     |-- upload ------->|                |                 |                    |
     |                  |                |                 |                    |
     |-- URL + verifiers --------------->|--- relay ------>|                    |
     |                  |                |                 |-- fetch & decrypt  |
     |                  |<------- fetch encrypted ---------|                    |
     |                  |-------- ciphertext ------------->|                    |
     |                  |                |                 |-- verify C2PA      |
     |                  |                |                 |-- build DAG        |
     |                  |                |                 |-- run WASM         |
     |                  |                |                 |-- sign results     |
     |<-- encrypted results ------------|<----------------|                    |
     |                                                                         |
     |  decrypt, upload signed_json to Arweave                                 |
     |  POST /sign → TEE builds partial TX → client co-signs and broadcasts    |
     |------------------------------------------------------------------------>|
```

Registration is split into two phases — **Verify** and **Sign**. In Verify, the TEE processes the content and signs the results. The client then uploads the signed result to permanent storage (Arweave). In Sign, the TEE builds a cNFT mint transaction and partially signs it; the client co-signs with their wallet and broadcasts to Solana. This split keeps the TEE stateless and avoids giving it a Solana wallet or dependency on external state.

## Two Layers

The protocol has two layers that share the same registration flow:

| | Core | Extension |
|---|---|---|
| Question | *What content was used to create this?* | *What are the properties of this content?* |
| Shared steps | C2PA signature verification → content_hash derivation | *(same)* |
| Divergence | Extract ingredient relationships → provenance DAG | Run WASM module → key-value attributes |
| Output | Content family tree (who-used-what graph) | Objective attributes (phash, license, etc.) |
| cNFT Collection | Core Collection | Extension Collection |

Both layers start from the same C2PA verification. They diverge at step 3: Core extracts ingredient relationships from the C2PA manifest to build a provenance DAG, while Extension runs a WASM module to compute attributes.

**Core** builds a provenance graph — a DAG where nodes are content_hash values (SHA-256 of each Active Manifest's signature) and edges represent "used as ingredient" relationships. The graph records content relationships; the current owner of each node is resolved separately at query time by looking up cNFTs on-chain.

**Extension** runs deterministic WASM modules against the raw content to produce objective attributes. Any WASM binary can be registered — the DAO maintains an on-chain allowlist (`trusted_wasm_modules` in GlobalConfig) of approved module URIs and their SHA-256 hashes. The TEE fetches the binary from the registered URI, verifies its hash, and executes it in a sandboxed wasmtime runtime.

This repository includes four reference modules:

| Module | Output |
|--------|--------|
| `phash-v1` | Perceptual hash for similarity search |
| `hardware-google` | Hardware capture proof (Titan M2 chip detection) |
| `c2pa-training-v1` | AI training consent flag (`c2pa.training-mining`) |
| `c2pa-license-v1` | License information (Creative Commons, rights) |

## Trust Model

The protocol's sole trust assumption is a single on-chain account — the **GlobalConfig PDA** — controlled by a DAO multi-sig. It designates the official cNFT collections and the authorized TEE nodes. The DAO delegates Collection Authority to trusted TEE nodes, so any cNFT minted into the official collections was necessarily issued by an authorized TEE.

```
GlobalConfig (DAO)           ← Trust root
  → Official Collections     ← Collection Authority delegated to trusted TEEs
    → cNFT                   ← On-chain record (minted by authorized TEE)
      → Off-chain Data       ← TEE-signed, tamper-evident
        → Verified Content     Attribution
```

Anyone can verify any record by following this chain. No trust in the protocol operator is required — only trust in the DAO's governance, which is fully transparent on-chain.

## Design Principles

- **Content-Agnostic** — The protocol is a registry, not a regulator. Node operators cannot see the raw content (E2EE).
- **Stateless** — TEE nodes hold no state between requests. Keys are ephemeral and lost on restart.
- **Permissionless** — Anyone can build on the protocol or run a node. The canonical network is DAO-governed.
- **Smart-Contract-Less** — No custom mint logic. Uses Metaplex standards (Bubblegum cNFT + MPL Core Collections) directly.

## Vendor Separation

The protocol core is **vendor-neutral**. All vendor-specific code is isolated behind Cargo feature flags.

| | Protocol Core | AWS Vendor (`vendor-aws`) | Local Dev (`vendor-local`) |
|---|---|---|---|
| TEE Runtime | `TeeRuntime` trait | Nitro Enclaves (NSM API) | MockRuntime |
| Temp Storage | `TempStorage` trait | S3-compatible | Local HTTP file server |
| Transport | — | vsock (Nitro) | TCP |
| Deploy | — | `deploy/aws/` | `deploy/local/` |

```bash
# Protocol core only (no vendor dependencies)
cargo check --workspace --no-default-features

# With AWS vendor implementation (default)
cargo check --workspace
```

To add a new vendor, implement the `TeeRuntime` and `TempStorage` traits and create a `deploy/<vendor>/` directory.

## Repository Structure

```
crates/
  types/          — Shared type definitions
  crypto/         — ECDH, HKDF, AES-GCM, Ed25519, attestation verification
  core/           — C2PA verification & provenance graph construction
  wasm-host/      — WASM execution engine (wasmtime, fuel/memory limits)
  tee/            — TEE server: /verify, /sign, /register-node
  gateway/        — Gateway HTTP server: upload, relay, sign-and-mint
  proxy/          — HTTP proxy for TEE network isolation
  cli/            — CLI: init-global, register-node, create-tree, remove-node
wasm/             — WASM modules (no_std): phash-v1, hardware-google, c2pa-training-v1, c2pa-license-v1
programs/
  title-config/   — Anchor program: GlobalConfig + TeeNodeAccount PDA management
sdk/ts/           — TypeScript client SDK: E2EE, register, resolve
indexer/          — cNFT indexer: webhook + poller + DAS API → PostgreSQL
docker/           — Container images (Gateway, TEE, Proxy, Indexer)
deploy/           — Vendor-specific deployment (local, AWS Nitro)
integration-tests/ — E2E tests with real C2PA-signed fixtures
```

## Quick Start

**[QUICKSTART.md](QUICKSTART.md)** — Full walkthrough: deploy your own GlobalConfig on devnet, run a local node, and register C2PA-signed content on-chain.

```bash
# Build and test the Rust workspace
cargo check --workspace
cargo test --workspace

# Build WASM modules
for dir in wasm/*/; do
  (cd "$dir" && cargo build --target wasm32-unknown-unknown --release)
done

# Build TypeScript SDK & Indexer
cd sdk/ts && npm install && npm run build && cd ../..
cd indexer && npm install && npm run build && cd ..
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, coding standards, and pull request guidelines.

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).
