# deploy/local — Local Node Deployment

ローカル開発環境で Title Protocol ノードを起動する完全手順。

すべてのプロセスがホスト上で直接動作する（Docker は PostgreSQL のみ）。

> AWS 本番デプロイは [`deploy/aws/README.md`](../aws/README.md) を参照。

---

## Architecture

```
Client --> Gateway (:3000) --> TempStorage (:3001) --> TEE (:4000) --> Solana
                                                       |
                                                  WASM Modules
                                                  (phash, etc.)

PostgreSQL (:5432) <-- Indexer (:5001)
```

| Process | Port | Role |
|---------|------|------|
| `title-tee` | 4000 | TEE（MockRuntime — C2PA 検証、WASM 実行） |
| `title-temp-storage` | 3001 | 一時ファイルストレージ |
| `title-gateway` | 3000 | クライアント向け HTTP API |
| `indexer` | 5001 | cNFT インデクサ（オプション） |
| `postgres` | 5432 | インデクサ用 DB（Docker） |

---

## Prerequisites

| Tool | Required | Notes |
|------|----------|-------|
| [Rust](https://rustup.rs/) + `wasm32-unknown-unknown` target | Yes | `rustup target add wasm32-unknown-unknown` |
| [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) v2.0+ | Yes | |
| [Docker](https://docs.docker.com/get-docker/) (with Compose V2) | Yes | PostgreSQL 用 |
| [Python 3](https://www.python.org/) | Yes | `setup.sh` が `network.json` の解析に使用 |
| [Node.js](https://nodejs.org/) 20+ | Optional | Indexer 用。未インストール時はスキップされる |
| ~0.6 SOL on devnet | Yes | ノード登録 + Merkle Tree 作成に必要 |
| `network.json` | Yes | Phase 1 で作成。→ [`programs/title-config/README.md`](../../programs/title-config/README.md) |

---

## Setup

```bash
# 1. .env を作成
cp .env.example .env
# SOLANA_RPC_URL は .env.example にデフォルト値あり。通常はそのままでOK。

# 2. 起動（全自動）
./deploy/local/setup.sh
```

> **初回ビルドには 10〜20 分かかる。** 2回目以降はキャッシュが効く。

### What `setup.sh` Does

| Step | What | Details |
|------|------|---------|
| 0 | Prerequisite check | Rust, Solana CLI, Docker, .env, network.json, SOL balance を検証 |
| 1 | Build WASM modules | 4 modules → `wasm-modules/` |
| 2 | Build host binaries | TEE, Gateway, TempStorage, CLI |
| 3 | Start TEE | MockRuntime, port 4000 |
| 4 | Start services | TempStorage (:3001), Gateway (:3000), PostgreSQL (:5432), Indexer (:5001) |
| 5 | Register TEE node | オンチェーンノード登録（`keys/authority.json` 存在時は自動署名） |
| 6 | Create Merkle Trees | Core + Extension trees for cNFT minting |
| 7 | Health check | 全サービスの応答確認 |

### Auto-configured Values

以下の値は `setup.sh` が自動設定する（手動設定不要）:

| Value | Source |
|-------|--------|
| `GATEWAY_SIGNING_KEY` | `openssl rand -hex 32` で自動生成 |
| `CORE_COLLECTION_MINT` | `network.json` → `core_collection_mint` |
| `EXT_COLLECTION_MINT` | `network.json` → `ext_collection_mint` |
| `GLOBAL_CONFIG_PDA` | `network.json` → `global_config_pda` |
| `keys/operator.json` | `~/.config/solana/id.json` からコピー、または自動生成 |

> `.env` で明示設定した場合はそちらが優先される。全環境変数の詳細は [docs/reference.md](../../docs/reference.md) を参照。

---

## Verify the Node

ノード起動後、テスト写真で動作確認:

```bash
# SDK をビルド（integration-tests が依存）
cd sdk/ts && npm install && npm run build && cd ../..

# integration-tests から verify
cd integration-tests && npm install
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --skip-sign

# Full flow: verify + Arweave upload + cNFT mint
npx tsx register-photo.ts localhost ./fixtures/pixel_photo_ramen.jpg \
  --wallet ../keys/operator.json --broadcast
```

See also:

- `integration-tests/stress-test.ts` — Concurrent registration under load
- `integration-tests/fixtures/` — Sample C2PA-signed images for testing

---

## Logs

```bash
tail -f /tmp/title-tee.log
tail -f /tmp/title-temp-storage.log
tail -f /tmp/title-gateway.log
tail -f /tmp/title-indexer.log
```

---

## Restart Individual Processes

```bash
# 例: Gateway だけ再起動
kill $(cat /tmp/title-local/gateway.pid)
TEE_ENDPOINT=http://localhost:4000 \
  LOCAL_STORAGE_ENDPOINT=http://localhost:3001 \
  GATEWAY_SIGNING_KEY=<hex from setup.sh output> \
  SOLANA_RPC_URL=https://api.devnet.solana.com \
  GLOBAL_CONFIG_PDA=<from network.json> \
  ./target/release/title-gateway
```

PID ファイルは `/tmp/title-local/` に保存される:

| PID File | Process |
|----------|---------|
| `tee.pid` | `title-tee` |
| `temp-storage.pid` | `title-temp-storage` |
| `gateway.pid` | `title-gateway` |
| `indexer.pid` | `indexer` |

---

## Stop Everything

```bash
./deploy/local/teardown.sh
```

---

## Troubleshooting

See [docs/troubleshooting.md](../../docs/troubleshooting.md).
