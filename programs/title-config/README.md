# Title Config Program — Phase 1: Network Setup

Deploy the Anchor program and initialize GlobalConfig. **Run once per developer.**

Phase 1 produces `network.json`, which is consumed by Phase 2 ([local node](../../deploy/local/README.md) / [AWS node](../../deploy/aws/README.md)).

> For a conceptual overview, see [docs/architecture.md](../../docs/architecture.md).

---

## Prerequisites

| Tool | Notes |
|------|-------|
| [Rust](https://rustup.rs/) + `wasm32-unknown-unknown` target | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | |
| `cargo-build-sbf` | Bundled with Solana CLI |
| ~5 SOL on devnet | Program deploy costs ~2 SOL. [faucet.solana.com](https://faucet.solana.com) or `solana airdrop` |

---

## Step 1: Generate Program Keypair

Each developer deploys their own program instance on devnet. This ensures complete isolation — your own GlobalConfig PDA, your own collections, your own authority.

```bash
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json --force --no-bip39-passphrase
solana-keygen pubkey programs/title-config/target/deploy/title_config-keypair.json
# Note this Program ID — you'll need it in the next step.
```

## Step 2: Update `declare_id!`

Update the Program ID in all of these files:

| File | Location |
|------|----------|
| `programs/title-config/src/lib.rs` | `declare_id!("...")` |
| `Anchor.toml` | `[programs.localnet]` and `[programs.devnet]` |
| `crates/cli/src/commands/init_global.rs` | `DEFAULT_PROGRAM_ID` |
| `crates/cli/src/anchor.rs` | test program IDs |
| `crates/tee/src/endpoints/register_node.rs` | test program IDs |
| `sdk/ts/src/chain.ts` | `TITLE_CONFIG_PROGRAM_ID` |
| `crates/tee/src/main.rs` | `PROGRAM_ID` env var fallback default |

## Step 3: Build

```bash
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
cd ../..
```

## Step 4: Deploy

```bash
solana program deploy programs/title-config/target/deploy/title_config.so \
  --program-id programs/title-config/target/deploy/title_config-keypair.json \
  --url devnet
```

> Deploys using your Solana CLI default wallet as payer. Needs ~5 SOL (program deploy costs ~2 SOL, the remainder for later operations).

## Step 5: Build WASM Modules

```bash
for dir in wasm/*/; do
  (cd "$dir" && cargo build --target wasm32-unknown-unknown --release)
done
```

## Step 6: Build the CLI

```bash
cargo build --release -p title-cli
```

## Step 7: Initialize GlobalConfig

```bash
./target/release/title-cli init-global --cluster devnet
```

This is **idempotent** — safe to run multiple times. It will:

1. Load or create an authority keypair at `keys/authority.json`
2. Create two MPL Core Collections (Core + Extension) if not already present
3. Call `initialize` to create the GlobalConfig PDA (skipped if it already exists)
4. Set default ResourceLimits on-chain via `set_resource_limits` (file size caps, timeouts, etc.)
5. Write `network.json` to the project root

WASM modules are registered separately via `title-cli register-wasm` (see Step 8 below).

Both `keys/authority.json` and `network.json` are gitignored — they are local to your environment.

## Step 8: Register WASM Modules

Register WASM modules on-chain with `title-cli register-wasm`. Each module gets a WasmModuleAccount PDA storing its SHA-256 hash and source URL.

```bash
./target/release/title-cli register-wasm \
  --extension-id image-pdq \
  --wasm-path wasm/image-pdq/target/wasm32-unknown-unknown/release/image_pdq.wasm
./target/release/title-cli register-wasm \
  --extension-id image-phash \
  --wasm-path wasm/image-phash/target/wasm32-unknown-unknown/release/image_phash.wasm
./target/release/title-cli register-wasm \
  --extension-id video-vpdq \
  --wasm-path wasm/video-vpdq/target/wasm32-unknown-unknown/release/video_vpdq.wasm
./target/release/title-cli register-wasm \
  --extension-id cert-google \
  --wasm-path wasm/cert-google/target/wasm32-unknown-unknown/release/cert_google.wasm
./target/release/title-cli register-wasm \
  --extension-id cert-sony \
  --wasm-path wasm/cert-sony/target/wasm32-unknown-unknown/release/cert_sony.wasm
./target/release/title-cli register-wasm \
  --extension-id cert-leica \
  --wasm-path wasm/cert-leica/target/wasm32-unknown-unknown/release/cert_leica.wasm
./target/release/title-cli register-wasm \
  --extension-id cert-rootlens \
  --wasm-path wasm/cert-rootlens/target/wasm32-unknown-unknown/release/cert_rootlens.wasm
```

| Extension ID | WASM Module Directory | Description |
|-------------|----------------------|-------------|
| `image-pdq` | `wasm/image-pdq` | PDQ 256-bit perceptual hash |
| `image-phash` | `wasm/image-phash` | pHash 64-bit perceptual hash (deprecated) |
| `video-vpdq` | `wasm/video-vpdq` | vPDQ per-frame video hash |
| `cert-google` | `wasm/cert-google` | Google C2PA certificate chain verification |
| `cert-sony` | `wasm/cert-sony` | Sony C2PA certificate chain verification |
| `cert-leica` | `wasm/cert-leica` | Leica C2PA certificate chain verification |
| `cert-rootlens` | `wasm/cert-rootlens` | RootLens C2PA certificate chain verification |

## Step 9: Collection Authority Delegation (automatic)

For TEE nodes to mint cNFTs, collection Authority must be delegated to the TEE's `signing_pubkey`.

**This delegation happens automatically during `register-node`.** The `register_tee_node` Anchor instruction executes MPL Core CPI internally, completing GlobalConfig registration and collection authority delegation atomically in a single transaction.

**Invariant:** `GlobalConfig.trusted_node_keys == Collection UpdateDelegate.additional_delegates`

| Operation | Authority Delegation |
|-----------|---------------------|
| `register_tee_node` | MPL Core `AddCollectionPluginV1` (first node) / `UpdateCollectionPluginV1` (subsequent) |
| `remove_tee_node` | MPL Core `UpdateCollectionPluginV1` (nodes remaining) / `RemoveCollectionPluginV1` (last node) |

---

## Output: `network.json`

After Phase 1, `network.json` is generated in the project root. This file bridges Phase 1 and Phase 2.

`network.json` is for bootstrapping only. After initialization, the on-chain GlobalConfig becomes the single source of truth.

---

## Next: Phase 2 — Node Deployment

- **Local development:** [`deploy/local/README.md`](../../deploy/local/README.md)
- **AWS production:** [`deploy/aws/README.md`](../../deploy/aws/README.md)

---

## Program Instructions Reference

On-chain instructions provided by this program:

| Instruction | Description | Authority Required |
|------------|-------------|-------------------|
| `initialize` | Create GlobalConfig PDA | Yes |
| `register_tee_node` | Register TEE node + delegate collection authority (MPL Core CPI) | Yes |
| `remove_tee_node` | Remove TEE node + revoke collection authority (MPL Core CPI) | Yes |
| `register_wasm_module` | Create WasmModuleAccount PDA + add to `trusted_wasm_ids` + register initial version | Yes |
| `remove_wasm_module` | Close WasmModuleAccount PDA + remove from `trusted_wasm_ids` | Yes |
| `add_wasm_version` | Add new version to existing WasmModuleAccount (PDA realloc) | Yes |
| `set_resource_limits` | Set ResourceLimits | Yes |
