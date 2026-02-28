# deploy/local — ローカル開発環境

## アーキテクチャ

```
Client --> Gateway (:3000) --> TempStorage (:3001) --> TEE (:4000) --> Solana
                                                       |
                                                  WASM Modules
                                                  (phash, etc.)

PostgreSQL (:5432) <-- Indexer (:5000)
```

すべてのプロセスがホスト上で直接動作する（Dockerは PostgreSQL のみ）。
各プロセスが独立しているため、アーキテクチャの各コンポーネントが目に見える。

| プロセス | ポート | 役割 |
|---------|--------|------|
| `title-temp-storage` | 3001 | 一時ファイルストレージ |
| `title-gateway` | 3000 | クライアント向けHTTP API |
| `title-tee` | 4000 | TEE（C2PA検証、WASM実行） |
| `indexer` | 5000 | cNFTインデクサ |
| `postgres` | 5432 | インデクサ用DB（Docker） |

## 前提条件

- [Rust](https://rustup.rs/) + `wasm32-unknown-unknown` ターゲット
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (v2.0+)
- [Docker](https://docs.docker.com/get-docker/) (PostgreSQL用)
- [Node.js](https://nodejs.org/) 20+ (Indexer用、オプション)
- Phase 1 完了済み（`network.json` が存在すること）

## セットアップ

```bash
# 1. .env を作成（最小設定）
cp .env.example .env
# SOLANA_RPC_URL を設定するだけでOK

# 2. 起動
./deploy/local/setup.sh

# 3. 停止
./deploy/local/teardown.sh
```

## ログの確認

```bash
tail -f /tmp/title-tee.log
tail -f /tmp/title-temp-storage.log
tail -f /tmp/title-gateway.log
tail -f /tmp/title-indexer.log
```

## 個別プロセスの再起動

```bash
# 例: Gateway だけ再起動
kill $(cat /tmp/title-local/gateway.pid)
TEE_ENDPOINT=http://localhost:4000 \
  LOCAL_STORAGE_ENDPOINT=http://localhost:3001 \
  SOLANA_RPC_URL=https://api.devnet.solana.com \
  ./target/release/title-gateway
```
