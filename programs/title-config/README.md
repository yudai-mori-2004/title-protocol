# Title Config Program — Phase 1: Network Setup

Title Protocol の Anchor プログラムをデプロイし、GlobalConfig を初期化する手順。**開発者ごとに1回だけ実行する。**

Phase 1 で `network.json` を生成し、それを Phase 2（[ローカルノード](../../deploy/local/README.md) / [AWS ノード](../../deploy/aws/README.md)）で使用する。

> アーキテクチャの概念説明は [docs/architecture.md](../../docs/architecture.md) を参照。

---

## Prerequisites

| Tool | Notes |
|------|-------|
| [Rust](https://rustup.rs/) + `wasm32-unknown-unknown` target | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | |
| `cargo-build-sbf` | Solana CLI に同梱 |
| ~5 SOL on devnet | Program deploy に ~2 SOL。[faucet.solana.com](https://faucet.solana.com) or `solana airdrop` |

---

## Step 1: Generate Program Keypair

Each developer deploys their own program instance on devnet. This ensures complete isolation — your own GlobalConfig PDA, your own collections, your own authority.

```bash
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json --force
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
4. Register the 4 built-in WASM modules via `add_wasm_module` (upsert — updates hash if already registered)
5. Set default ResourceLimits on-chain via `set_resource_limits` (file size caps, timeouts, etc.)
6. Write `network.json` to the project root

Both `keys/authority.json` and `network.json` are gitignored — they are local to your environment.

## Step 8: Collection Authority Delegation (自動)

TEE ノードが cNFT をミントするには、コレクションの Authority 権限を TEE の `signing_pubkey` に委譲する必要がある。

**この委譲は `register-node` 時に自動的に行われる。** `register_tee_node` Anchor 命令内で MPL Core CPI が実行され、GlobalConfig への登録とコレクション権限委譲が 1 トランザクションで不可分に完了する。

**不変条件:** `GlobalConfig.trusted_node_keys == コレクションの UpdateDelegate.additional_delegates`

| 操作 | 権限委譲 |
|------|---------|
| `register_tee_node` | MPL Core `AddCollectionPluginV1`（初回）/ `UpdateCollectionPluginV1`（追加） |
| `remove_tee_node` | MPL Core `UpdateCollectionPluginV1`（残ノードあり）/ `RemoveCollectionPluginV1`（最後） |

---

## Output: `network.json`

Phase 1 完了後、プロジェクトルートに `network.json` が生成される。このファイルが Phase 2 への橋渡し。

フィールド詳細は [docs/reference.md — network.json Schema](../../docs/reference.md#networkjson-schema) を参照。

---

## Next: Phase 2 — Node Deployment

- **ローカル開発:** [`deploy/local/README.md`](../../deploy/local/README.md)
- **AWS 本番:** [`deploy/aws/README.md`](../../deploy/aws/README.md)

---

## Program Instructions Reference

このプログラムが提供する on-chain instructions:

| Instruction | Description | Authority Required |
|------------|-------------|-------------------|
| `initialize` | GlobalConfig PDA を作成 | Yes |
| `register_tee_node` | TEE ノードを登録 + コレクション権限委譲（MPL Core CPI） | Yes |
| `remove_tee_node` | TEE ノードを削除 + コレクション権限取り消し（MPL Core CPI） | Yes |
| `add_wasm_module` | WASM モジュールを登録（upsert） | Yes |
| `set_resource_limits` | ResourceLimits を設定 | Yes |
