# Reference

環境変数、`network.json` スキーマ、CLI コマンドのリファレンス。

---

## Environment Variables

すべての環境変数は [`.env.example`](../.env.example) に定義されている。`deploy/local/setup.sh` を使う場合、`SOLANA_RPC_URL` のみ手動設定すれば他は自動設定される。

### Common

| Variable | Required | Description |
|----------|----------|-------------|
| `SOLANA_RPC_URL` | Yes | Solana RPC endpoint. ローカル開発では `https://api.devnet.solana.com`。本番では dedicated RPC 推奨（Helius 等）。 |

### Gateway (`crates/gateway`)

| Variable | Required | Description |
|----------|----------|-------------|
| `GATEWAY_SIGNING_KEY` | Auto | Ed25519 secret key (64-char hex). `setup.sh` が未設定時に自動生成する。Gateway と TEE が同じ鍵を共有する必要がある。 |
| `TEE_ENDPOINT` | Auto | TEE server URL. Default: `http://localhost:4000`. |
| `GLOBAL_CONFIG_PDA` | Auto | GlobalConfig PDA address. Gateway 起動時にオンチェーン ResourceLimits を取得する。`network.json` から自動設定。 |
| `GATEWAY_SOLANA_KEYPAIR` | No | Solana keypair (Base58) for `/sign-and-mint` (delegateMint). Operator がクライアントに代わって TX 手数料を支払う。`delegateMint: true` 使用時のみ必要。 |

### Gateway — TempStorage (vendor-aws)

| Variable | Required | Description |
|----------|----------|-------------|
| `S3_ENDPOINT` | AWS only | S3-compatible API endpoint (MinIO, R2, etc.) |
| `S3_PUBLIC_ENDPOINT` | No | Client-facing endpoint. Default: `S3_ENDPOINT` と同値。 |
| `S3_ACCESS_KEY` | AWS only | Storage access key |
| `S3_SECRET_KEY` | AWS only | Storage secret key |
| `S3_BUCKET` | AWS only | Bucket name for temp uploads |
| `S3_REGION` | No | S3 region |

### Gateway — TempStorage (vendor-local)

| Variable | Required | Description |
|----------|----------|-------------|
| `LOCAL_STORAGE_ENDPOINT` | Auto | TEE-facing endpoint. Default: `http://localhost:3001`. |
| `LOCAL_STORAGE_PUBLIC_ENDPOINT` | No | Client-facing endpoint. Default: `LOCAL_STORAGE_ENDPOINT` と同値。 |
| `STORAGE_DIR` | No | TempStorage server file directory. Default: `/tmp/title-uploads`. |
| `STORAGE_PORT` | No | TempStorage server port. Default: `3001`. |

### TEE (`crates/tee`)

| Variable | Required | Description |
|----------|----------|-------------|
| `TEE_RUNTIME` | Auto | Runtime implementation. `mock`（ローカル）or vendor runtime（`nitro` 等）。`setup.sh` が自動設定。 |
| `PROXY_ADDR` | Auto | `direct`（直接 HTTP）or `127.0.0.1:8000`（vsock bridge, Enclave 内部）。 |
| `CORE_COLLECTION_MINT` | Auto | Core cNFT Collection Mint address. **`network.json` から自動読み取り。** `.env` で明示設定した場合はそちらが優先。 |
| `EXT_COLLECTION_MINT` | Auto | Extension cNFT Collection Mint address. **`network.json` から自動読み取り。** `.env` で明示設定した場合はそちらが優先。 |
| `GATEWAY_PUBKEY` | No | Gateway 認証用 Ed25519 public key (Base58). 未設定時は Gateway 認証をスキップ（開発環境用）。 |
| `TRUSTED_EXTENSIONS` | Auto | 信頼する WASM Extension のカンマ区切りリスト. Default: `phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1`. |
| `WASM_DIR` | Auto | WASM バイナリディレクトリ. Default: `/wasm-modules`. |

### Proxy (`crates/proxy`)

| Variable | Required | Description |
|----------|----------|-------------|
| *(automatic)* | — | Production: vsock port 8000 (vendor-aws). Development: TCP `127.0.0.1:8000`. |

### Indexer (`indexer/`)

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Auto | PostgreSQL connection string. Default: `postgres://title:title_dev@localhost:5432/title_indexer`. |
| `DAS_ENDPOINTS` | Auto | DAS API endpoints (comma-separated). Default: `SOLANA_RPC_URL` と同値。 |
| `COLLECTION_MINTS` | Auto | 監視対象 Collection Mint のカンマ区切りリスト. `setup.sh` が `network.json` から自動設定。 |
| `WEBHOOK_PORT` | No | Webhook listen port. Default: `5001`. |
| `WEBHOOK_SECRET` | No | Webhook auth secret. |

### Docker Compose

| Variable | Required | Description |
|----------|----------|-------------|
| `DB_USER` | No | PostgreSQL user. Default: `title`. |
| `DB_PASSWORD` | Production | PostgreSQL password. 本番環境では必須。 |

---

## `network.json` Schema

`title-cli init-global` が生成するブートストラップファイル。Phase 2 の `setup.sh` / `setup-ec2.sh` がこのファイルから設定値を読み取る。

```json
{
  "cluster": "devnet",
  "program_id": "<Base58 Program ID>",
  "global_config_pda": "<Base58 GlobalConfig PDA address>",
  "authority": "<Base58 authority pubkey>",
  "core_collection_mint": "<Base58 Core Collection Mint address>",
  "ext_collection_mint": "<Base58 Extension Collection Mint address>",
  "wasm_modules": {
    "phash-v1": { "hash": "<SHA-256 hex>" },
    "hardware-google": { "hash": "<SHA-256 hex>" },
    "c2pa-training-v1": { "hash": "<SHA-256 hex>" },
    "c2pa-license-v1": { "hash": "<SHA-256 hex>" }
  }
}
```

| Field | Writer | Reader | Description |
|-------|--------|--------|-------------|
| `cluster` | `init-global` | `setup.sh` | Solana cluster (`devnet` / `mainnet`) |
| `program_id` | `init-global` | `register-node`, `create-tree` | Deployed title-config program ID |
| `global_config_pda` | `init-global` | `setup.sh` → Gateway `GLOBAL_CONFIG_PDA` | GlobalConfig PDA address |
| `authority` | `init-global` | `setup.sh` | Authority pubkey (for display) |
| `core_collection_mint` | `init-global` | `setup.sh` → TEE `CORE_COLLECTION_MINT`, `register-node` | Core cNFT Collection address |
| `ext_collection_mint` | `init-global` | `setup.sh` → TEE `EXT_COLLECTION_MINT`, `register-node` | Extension cNFT Collection address |
| `wasm_modules` | `init-global` | — | Registered WASM module hashes (reference) |

> **Note:** `network.json` はブートストラップ用。初期化後は、オンチェーン GlobalConfig が唯一の信頼できるソースとなる。

---

## CLI Commands (`title-cli`)

### `init-global`

GlobalConfig PDA の初期化、MPL Core コレクション作成、WASM モジュール登録を行う。冪等（何度実行しても安全）。

```bash
title-cli init-global --cluster devnet [--rpc <URL>] [--program-id <PUBKEY>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--cluster` | `devnet` | Solana cluster (`devnet` / `mainnet`) |
| `--rpc` | cluster default | Solana RPC URL |
| `--program-id` | built-in | title-config program ID |

**実行内容:**
1. `keys/authority.json` をロードまたは新規作成
2. Core / Extension MPL Core Collection を作成（未作成の場合）
3. `initialize` で GlobalConfig PDA を作成（既存の場合はスキップ）
4. 4つの WASM モジュールを `add_wasm_module` で登録（upsert）
5. `set_resource_limits` でデフォルトの ResourceLimits を設定
6. `network.json` を出力

### `register-node`

TEE ノードをオンチェーンに登録する。

```bash
title-cli register-node \
  --tee-url http://localhost:4000 \
  --gateway-endpoint http://localhost:3000 \
  [--measurements '{"PCR0":"...","PCR1":"...","PCR2":"..."}']
```

| Flag | Default | Description |
|------|---------|-------------|
| `--tee-url` | `http://localhost:4000` | TEE server URL |
| `--gateway-endpoint` | `http://localhost:3000` | Gateway の外部公開エンドポイント |
| `--measurements` | — | TEE 測定値 (JSON, Nitro Enclave のみ) |

**動作:**
- TEE `/register-node` を呼び出し、TEE が部分署名済み TX を返す
- `keys/authority.json` が存在すれば自動で共同署名 + ブロードキャスト
- 存在しなければ部分署名 TX を表示（DAO 承認待ち）

`register_tee_node` 命令は GlobalConfig への登録と同時に、MPL Core CPI で両コレクション（Core + Extension）の UpdateDelegate プラグインに TEE の signing_pubkey を追加する。コレクション権限委譲は登録と不可分に1トランザクションで完了する。

### `create-tree`

Core + Extension の Merkle Tree を作成する。

```bash
title-cli create-tree \
  --tee-url http://localhost:4000 \
  [--max-depth 14] \
  [--max-buffer-size 64]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--tee-url` | `http://localhost:4000` | TEE server URL |
| `--max-depth` | `14` | Merkle Tree の深さ（最大 2^14 = 16,384 leaves） |
| `--max-buffer-size` | `64` | 同時更新バッファサイズ |

### `remove-node`

TEE ノードをオンチェーンから削除する。`keys/authority.json` が必須。

```bash
title-cli remove-node --signing-pubkey <BASE58_PUBKEY>
```

| Flag | Required | Description |
|------|----------|-------------|
| `--signing-pubkey` | Yes | 削除するノードの signing pubkey (Base58) |

`remove_tee_node` 命令は GlobalConfig からの除去と同時に、MPL Core CPI で両コレクションの UpdateDelegate プラグインから signing_pubkey を削除する（最後のノード削除時はプラグイン自体を除去）。コレクション権限取消は削除と不可分に1トランザクションで完了する。

### Global Options

| Flag | Default | Description |
|------|---------|-------------|
| `--keys-dir` | `keys` | キーペアディレクトリ |

---

## Port Numbers

| Port | Service | Notes |
|------|---------|-------|
| 3000 | Gateway | Client-facing HTTP API |
| 3001 | TempStorage | ローカルのみ（AWS は S3 を使用） |
| 4000 | TEE | TEE server（ローカル直接 or socat bridge 経由） |
| 5001 | Indexer | Webhook listener |
| 5432 | PostgreSQL | Indexer 用 DB（Docker） |
| 8000 | Proxy | vsock bridge（Enclave 内部、AWS のみ） |
