# Quick Start

Get a Title Protocol node running locally on Solana devnet and verify a C2PA-signed photo.

> For architecture concepts, trust model, and wallet roles, see [docs/architecture.md](docs/architecture.md).

---

## Prerequisites

| Tool | Install |
|------|---------|
| [Rust](https://rustup.rs/) + wasm32 target | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | |
| [Docker](https://docs.docker.com/get-docker/) (with Compose V2) | |
| ~5 SOL on devnet | [faucet.solana.com](https://faucet.solana.com) or `solana airdrop 2 --url devnet` |

---

## Phase 1: Network Setup (one-time)

Build the Anchor program, deploy it, and initialize GlobalConfig. This creates `network.json`, which Phase 2 needs.

```bash
# 1. Generate program keypair
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json --force

# 2. Update declare_id! in programs/title-config/src/lib.rs (and 5 other files)
#    See programs/title-config/README.md for the full list.
solana-keygen pubkey programs/title-config/target/deploy/title_config-keypair.json

# 3. Build and deploy
cd programs/title-config && rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
cd ../..
solana program deploy programs/title-config/target/deploy/title_config.so \
  --program-id programs/title-config/target/deploy/title_config-keypair.json \
  --url devnet

# 4. Build WASM modules + CLI
for dir in wasm/*/; do (cd "$dir" && cargo build --target wasm32-unknown-unknown --release); done
cargo build --release -p title-cli

# 5. Initialize GlobalConfig (creates keys/authority.json + network.json)
./target/release/title-cli init-global --cluster devnet
```

> **Detailed guide:** [`programs/title-config/README.md`](programs/title-config/README.md) — includes declare_id! update locations, collection authority delegation, and network.json schema.

---

## Phase 2: Node Deployment

Start a local TEE node using `network.json` from Phase 1.

```bash
# 1. Create .env
cp .env.example .env

# 2. Start everything (builds, starts services, registers node, creates Merkle Trees)
./deploy/local/setup.sh
```

`setup.sh` handles WASM builds, binary compilation, service startup, node registration, and Merkle Tree creation. It auto-creates `keys/operator.json` and pauses for SOL funding if needed.

> **Detailed guide:** [`deploy/local/README.md`](deploy/local/README.md) — logs, individual process restart, auto-configured values.
>
> **AWS deployment:** [`deploy/aws/README.md`](deploy/aws/README.md) — Terraform, Nitro Enclaves, mainnet.

---

## Verify a Photo

```bash
# Build the SDK (integration-tests depend on it)
cd sdk/ts && npm install && npm run build && cd ../..

# Run the test
cd integration-tests && npm install
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --skip-sign
```

You should see a successful verification result with `content_hash`, `protocol: "Title-v1"`, and the provenance graph.

For the full flow (verify + Arweave upload + cNFT mint):

```bash
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --broadcast
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
