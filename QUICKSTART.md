# Quick Start

Get a Title Protocol node running locally on Solana devnet and verify a C2PA-signed photo.

> For architecture, trust model, and wallet roles, see [docs/architecture.md](docs/architecture.md).

---

## Prerequisites

| Tool | Install |
|------|---------|
| [Rust](https://rustup.rs/) + wasm32 target | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | Includes `cargo-build-sbf` and `solana-keygen` |
| [Docker](https://docs.docker.com/get-docker/) (with Compose V2) | |
| ~5 SOL on devnet | [faucet.solana.com](https://faucet.solana.com) or `solana airdrop 2 --url devnet` |

---

## Phase 1: Network Setup (one-time)

Build the on-chain program, deploy it, and initialize GlobalConfig. This produces `network.json`, which Phase 2 needs.

```bash
# 1. Generate a new program keypair
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json \
  --no-bip39-passphrase

# 2. Get the program ID from the keypair
solana-keygen pubkey programs/title-config/target/deploy/title_config-keypair.json
#    Copy the output and replace the program ID in all 6 files listed in
#    programs/title-config/README.md (declare_id!, Anchor.toml, SDK, CLI, TEE).

# 3. Build the on-chain program
cd programs/title-config && rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
cd ../..

# 4. Deploy to devnet
solana program deploy programs/title-config/target/deploy/title_config.so \
  --program-id programs/title-config/target/deploy/title_config-keypair.json \
  --url devnet

# 5. Build WASM modules + CLI
for dir in wasm/*/; do (cd "$dir" && cargo build --target wasm32-unknown-unknown --release); done
cargo build --release -p title-cli

# 6. Initialize GlobalConfig (creates keys/authority.json + network.json)
./target/release/title-cli init-global --cluster devnet
```

> **Full details:** [`programs/title-config/README.md`](programs/title-config/README.md) — program ID update locations, network.json schema, and what init-global does.

---

## Phase 2: Node Deployment

Start a local TEE node using `network.json` from Phase 1.

```bash
# 1. Create .env (defaults work for local devnet)
cp .env.example .env

# 2. Start everything
./deploy/local/setup.sh
```

`setup.sh` is fully automated. It builds binaries, starts all services, registers the TEE node on-chain, and creates Merkle Trees. If `keys/operator.json` doesn't exist, it creates one and pauses for SOL funding.

What `setup.sh` does:

| Step | Action |
|------|--------|
| 0 | Check prerequisites (Rust, Solana CLI, Docker, .env, network.json, SOL balance) |
| 1 | Build 4 WASM modules |
| 2 | Build host binaries (TEE, Gateway, TempStorage, CLI) |
| 3 | Start TEE (MockRuntime, port 4000) |
| 4 | Start services (TempStorage :3001, Gateway :3000, PostgreSQL :5432, Indexer :5001) |
| 5 | Register TEE node on-chain (auto-signs if `keys/authority.json` exists) |
| 6 | Create Merkle Trees (Core + Extension, for cNFT minting) |
| 7 | Health check all services |

> **AWS deployment:** [`deploy/aws/README.md`](deploy/aws/README.md) — Terraform, Nitro Enclaves, mainnet.

---

## Verify a Photo

```bash
# Build the SDK (integration-tests depend on it)
cd sdk/ts && npm install && npm run build && cd ../..

# Run verification only
cd integration-tests && npm install
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --skip-sign
```

You should see `protocol: "Title-v1"`, a `content_hash`, and the provenance graph.

For the full flow (verify + Arweave upload + cNFT mint):

```bash
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --broadcast
```

---

## Logs

```bash
tail -f /tmp/title-tee.log
tail -f /tmp/title-temp-storage.log
tail -f /tmp/title-gateway.log
tail -f /tmp/title-indexer.log
```

---

## Stop

```bash
./deploy/local/teardown.sh
```

---

## What's Next

| Goal | Guide |
|------|-------|
| Understand the architecture | [docs/architecture.md](docs/architecture.md) |
| Deploy on AWS with Nitro Enclaves | [deploy/aws/README.md](deploy/aws/README.md) |
| Run a mainnet node | [deploy/aws/README.md — Mainnet](deploy/aws/README.md#running-a-mainnet-node) |
| Build an app with the SDK | [sdk/ts/README.md](sdk/ts/README.md) |
| Query indexed cNFTs | [indexer/README.md](indexer/README.md) |
| Environment variables & CLI reference | [docs/reference.md](docs/reference.md) |
| Troubleshooting | [docs/troubleshooting.md](docs/troubleshooting.md) |
