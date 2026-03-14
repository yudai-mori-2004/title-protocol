# Title Protocol

**The Identity Layer for Digital Content**

---

## Getting Started

| You are a | You want to | Start here |
|---|---|---|
| **App developer** | Build apps that register or look up content on the official network | [SDK Guide](sdk/ts/README.md) |
| **Node operator** | Run a TEE node on the official network | [Local dev](deploy/local/README.md) · [AWS production](deploy/aws/README.md) |
| **Protocol deployer** | Stand up your own network with its own GlobalConfig | [Full deploy guide](QUICKSTART.md) |
| **Researcher** | Understand the architecture or read the full spec | [Architecture](docs/architecture.md) · [Technical Spec (JA)](docs/v0.1.0/SPECS_JA.md) |

---

## Why Title Protocol

Deepfakes erode trust in media. Unauthorized copies strip creators of credit and revenue. Digital evidence loses its weight in court. These are not separate problems confined to separate industries — they are the same structural gap manifesting across media, entertainment, finance, healthcare, law, and research. The root cause is the same: the internet has no reliable way to verify where content came from and determine who holds the rights to it.

### Two approaches to content provenance

Post-hoc detection — using AI classifiers or watermarks — is structurally limited. As generation technology improves, so does evasion. There is no stable asymmetry between the two.

The alternative is attaching cryptographic signatures at the time of creation. The security of these signatures (ECDSA, RSA, SHA-256) is independent of how realistic generated content becomes. No arms race.

### C2PA: the open standard

[C2PA](https://c2pa.org/) (Coalition for Content Provenance and Authenticity) implements this second approach as an open standard. It embeds a signed manifest into content — recording who created or edited it, with what tool, when, and from what source material. If the content is modified after signing, the signature breaks, making tampering detectable. Camera manufacturers, smartphone vendors, and generative AI providers already support C2PA, with over 6,000 organizations participating.

### What C2PA does not solve

C2PA makes provenance verifiable — but only for someone who holds the original file and runs verification locally. In practice, most people encounter content without holding the original: a photo on a social media timeline, a news image embedded in an article, a video recompressed and re-uploaded. Platforms routinely strip C2PA metadata. If original files were always shared intact, C2PA alone would suffice — but the internet does not work that way.

What is needed is a way for someone who does **not** hold the original file to look up whether it has been verified and who holds the rights — without trusting any single service's word. Yet no such public record exists. Verification results are ephemeral, stored nowhere, and disappear when the browser tab closes.

This leaves critical questions unanswered:

- A photographer's image appears in someone else's video on another platform. **Who is the rights holder, and where should payment go?**
- A news photo circulates on social media with its C2PA metadata stripped. **Can it be traced back to the original?**
- A service displays a "verified" badge. **Can a third party independently confirm the verification was performed correctly?**

Some services store verification results, but each maintains its own database with its own internal IDs — and merging them after the fact is impractical, since reconciling ID schemes, governance, and cost-sharing across competing services creates barriers that no single entity can resolve without concentrating power. Cross-platform rights tracking is structurally impossible. And because verification runs on each service's own servers, "it was correctly verified" must be taken on trust — no outside party can independently confirm it.

What is missing is not better verification technology. It is a **public, neutral registry** where verification results are recorded trustlessly and anyone can look them up.

---

## What Title Protocol Does

Title Protocol is that registry.

- A **TEE** (Trusted Execution Environment) performs C2PA verification in hardware-isolated memory. The node operator cannot see or tamper with the content or the verification process.
- The verification result is signed with the TEE's enclave-bound key and permanently recorded on the **Solana blockchain** as a compressed NFT (cNFT).
- The content's ID is derived deterministically from the C2PA manifest signature. **The same content always receives the same ID**, regardless of which service or node registers it.
- Anyone can look up a content ID on-chain and resolve it to the current rights holder's wallet address.

This achieves:

- **No silos** — every service and node shares the same ID space and the same public ledger.
- **Cross-platform rights tracking** — given a video, programmatically identify the rights holder of every image, audio clip, or other material used as an ingredient.
- **No trust in any single operator** — verification integrity is guaranteed by hardware isolation (TEE + Remote Attestation); record integrity by blockchain consensus.

### The Web infrastructure analogy

The internet relies on three layers of trust infrastructure to function. Digital content needs the same three — and until now, only the first existed.

| Layer | Web | Digital Content | Status |
|---|---|---|---|
| **Certificate** | SSL/TLS certificates + CAs | C2PA manifests + Trust List | Solved by C2PA |
| **Public ledger** | Certificate Transparency (CT) logs | TEE verification + blockchain recording | **Title Protocol** |
| **Name resolution** | DNS (domain → IP address) | content ID → rights holder wallet | **Title Protocol** |

C2PA is the "SSL/TLS certificate" for content — it provides the foundational cryptographic proof. Title Protocol provides the remaining two layers: a public, tamper-proof ledger of verification results, and a resolution system that maps content IDs to their current rights holders.

One structural difference from CT logs: CT logs perform only minimal format checks before recording, because X.509 certificates are a few kilobytes. C2PA content ranges from megabytes to gigabytes and requires full binary parsing, certificate chain validation, and content hash matching. Title Protocol therefore performs **substantive pre-verification in the TEE** before recording — only content that passes verification enters the ledger.

For a detailed treatment of this analogy, see [Architecture](docs/architecture.md).

---

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

Registration is split into two phases — **Verify** and **Sign**. In Verify, the TEE processes the content and signs the results. The client uploads the signed result to permanent storage (Arweave). In Sign, the TEE builds a cNFT mint transaction and partially signs it; the client co-signs with their wallet and broadcasts to Solana. This split keeps the TEE stateless and avoids giving it a Solana wallet or dependency on external state.

---

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

---

## Trust Model

The protocol's sole trust assumption is a single on-chain account — the **GlobalConfig PDA** — controlled by a DAO multi-sig. It designates the official cNFT collections and the authorized TEE nodes. The DAO delegates Collection Authority to trusted TEE nodes, so any cNFT minted into the official collections was necessarily issued by an authorized TEE.

```
GlobalConfig (DAO)              ← Trust root
  → Official Collections        ← Authority delegated to TEE nodes
    → cNFT                      ← On-chain record (minted by authorized TEE)
      → Off-chain signed_json   ← TEE-signed verification result
```

Anyone can verify any record by following this chain. No trust in the protocol operator is required — only trust in the DAO's governance, which is fully transparent on-chain.

---

## Design Principles

- **Content-Agnostic** — The protocol is a registry, not a regulator. Node operators cannot see the raw content (E2EE).
- **Stateless** — TEE nodes hold no state between requests. Keys are ephemeral and lost on restart.
- **Permissionless** — Anyone can build on the protocol or run a node. The canonical network is DAO-governed.
- **Smart-Contract-Less** — No custom mint logic. Uses Metaplex standards (Bubblegum cNFT + MPL Core Collections) directly.

---

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

---

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

---

## Build & Test

```bash
# Rust workspace
cargo check --workspace
cargo test --workspace

# WASM modules (built individually, outside workspace)
for dir in wasm/*/; do
  (cd "$dir" && cargo build --target wasm32-unknown-unknown --release)
done

# TypeScript SDK & Indexer
cd sdk/ts && npm install && npm run build && cd ../..
cd indexer && npm install && npm run build && cd ..
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [QUICKSTART.md](QUICKSTART.md) | Deploy your own GlobalConfig and full local network |
| [deploy/aws/README.md](deploy/aws/README.md) | AWS deployment with Nitro Enclaves |
| [docs/architecture.md](docs/architecture.md) | Architecture, trust model, and Web infrastructure analogy |
| [docs/reference.md](docs/reference.md) | Environment variables and CLI reference |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Common issues and solutions |
| [sdk/ts/README.md](sdk/ts/README.md) | TypeScript SDK guide |
| [indexer/README.md](indexer/README.md) | cNFT indexer setup |
| [programs/title-config/README.md](programs/title-config/README.md) | On-chain program details |
| [docs/v0.1.0/SPECS_JA.md](docs/v0.1.0/SPECS_JA.md) | Technical specification (Japanese, ~3000 lines) |

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, coding standards, and pull request guidelines.

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).
