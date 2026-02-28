#!/usr/bin/env bash
# Title Protocol ローカル開発 起動スクリプト
#
# すべてのプロセスをホスト上で直接起動する。
#
# 前提:
#   - network.json が存在する（title-cli init-global で事前に作成済み）
#   - .env が設定済み（SOLANA_RPC_URL のみ必須）
#   - Rust, Solana CLI, Node.js がインストール済み
#
# やること:
#   0. .env + network.json の読み込み・検証
#   1. WASMモジュールのビルド
#   2. ホスト側バイナリのビルド（TEE, Gateway, TempStorage, CLI）
#   3. TEE の起動
#   4. TempStorage + Gateway + PostgreSQL + Indexer の起動
#   5. TEEノード登録
#   6. Merkle Tree 作成
#   7. ヘルスチェック
#
# 使い方:
#   cd ~/title-protocol
#   ./deploy/local/setup.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_ROOT"

PID_DIR="/tmp/title-local"
mkdir -p "$PID_DIR"

echo "=== Title Protocol ローカル開発 起動 ==="

# ---------------------------------------------------------------------------
# Step 0: .env + network.json 読み込み・検証
# ---------------------------------------------------------------------------
echo "[Step 0/7] 設定ファイルの読み込み..."

# .env
if [ ! -f .env ]; then
  echo "ERROR: .env が見つかりません。.env.example をコピーして設定してください。"
  echo "  cp .env.example .env && vim .env"
  exit 1
fi

set -a
source .env
set +a

# network.json
NETWORK_JSON="$PROJECT_ROOT/network.json"
if [ ! -f "$NETWORK_JSON" ]; then
  echo "ERROR: network.json が見つかりません。"
  echo "  先に title-cli init-global でGlobalConfigを作成してください:"
  echo "    cargo build --release -p title-cli"
  echo "    ./target/release/title-cli init-global --cluster devnet"
  exit 1
fi

# network.json から値を読み取り
read_network() {
  python3 -c "import json,sys; d=json.load(open('$NETWORK_JSON')); print(d.get('$1',''))"
}

PROGRAM_ID=$(read_network "program_id")
GLOBAL_CONFIG_PDA=$(read_network "global_config_pda")
AUTHORITY_PUBKEY=$(read_network "authority")
CORE_COLLECTION_MINT_NET=$(read_network "core_collection_mint")
EXT_COLLECTION_MINT_NET=$(read_network "ext_collection_mint")
CLUSTER=$(read_network "cluster")

echo "  Cluster:            $CLUSTER"
echo "  Program ID:         $PROGRAM_ID"
echo "  GlobalConfig PDA:   $GLOBAL_CONFIG_PDA"
echo "  Authority:          $AUTHORITY_PUBKEY"
echo "  Core Collection:    $CORE_COLLECTION_MINT_NET"
echo "  Ext Collection:     $EXT_COLLECTION_MINT_NET"

# .env から未設定なら network.json の値を使う
CORE_COLLECTION_MINT="${CORE_COLLECTION_MINT:-$CORE_COLLECTION_MINT_NET}"
EXT_COLLECTION_MINT="${EXT_COLLECTION_MINT:-$EXT_COLLECTION_MINT_NET}"

# 必須変数の検証
REQUIRED_VARS=(
  SOLANA_RPC_URL
)

MISSING=()
for var in "${REQUIRED_VARS[@]}"; do
  if [ -z "${!var:-}" ]; then
    MISSING+=("$var")
  fi
done

if [ ${#MISSING[@]} -gt 0 ]; then
  echo "ERROR: 以下の必須環境変数が未設定です:"
  for var in "${MISSING[@]}"; do
    echo "  - $var"
  done
  exit 1
fi

echo "  .env: OK"

# Authority keypair の存在チェック
AUTHORITY_KEY_PATH="$PROJECT_ROOT/programs/title-config/keys/authority.json"
if [ -f "$AUTHORITY_KEY_PATH" ]; then
  echo "  Authority keypair: 検出 → 自動署名モード"
  AUTO_SIGN=true
else
  echo "  Authority keypair: なし → DAO承認モード"
  AUTO_SIGN=false
fi

# Solana ウォレットの確認
SOLANA_WALLET="$HOME/.config/solana/id.json"
if [ ! -f "$SOLANA_WALLET" ]; then
  echo "  Solana ウォレットが見つかりません。自動作成します..."
  solana-keygen new --no-bip39-passphrase -o "$SOLANA_WALLET"
fi
WALLET_PUBKEY=$(solana-keygen pubkey "$SOLANA_WALLET")
echo "  Solana ウォレット: $WALLET_PUBKEY"
solana config set --url "$SOLANA_RPC_URL" > /dev/null 2>&1 || true

# ---------------------------------------------------------------------------
# Step 1: WASMモジュールのビルド
# ---------------------------------------------------------------------------
echo ""
echo "[Step 1/7] WASMモジュールのビルド..."

WASM_OUTPUT="$PROJECT_ROOT/wasm-modules"
mkdir -p "$WASM_OUTPUT"

WASM_TARGETS=(phash-v1 hardware-google c2pa-training-v1 c2pa-license-v1)

for module in "${WASM_TARGETS[@]}"; do
  echo "  ビルド中: $module ..."
  (cd "wasm/$module" && cargo build --target wasm32-unknown-unknown --release)
  cp "wasm/$module/target/wasm32-unknown-unknown/release/${module//-/_}.wasm" "$WASM_OUTPUT/$module.wasm"
done
echo "  WASMモジュール → $WASM_OUTPUT/"

# ---------------------------------------------------------------------------
# Step 2: ホスト側バイナリのビルド
# ---------------------------------------------------------------------------
echo "[Step 2/7] ホスト側バイナリのビルド..."

echo "  title-tee をビルド中..."
cargo build --release --bin title-tee

echo "  title-cli をビルド中..."
cargo build --release --bin title-cli

echo "  title-temp-storage をビルド中..."
cargo build --release --bin title-temp-storage

echo "  title-gateway をビルド中 (vendor-local)..."
cargo build --release -p title-gateway --no-default-features --features vendor-local

echo "  ビルド完了"

# ---------------------------------------------------------------------------
# Step 3: TEE の起動
# ---------------------------------------------------------------------------
echo "[Step 3/7] TEE の起動..."

TEE_PID=$(pgrep -x title-tee 2>/dev/null || true)
if [ -z "$TEE_PID" ]; then
  TEE_RUNTIME=mock PROXY_ADDR=direct \
    SOLANA_RPC_URL="$SOLANA_RPC_URL" \
    CORE_COLLECTION_MINT="$CORE_COLLECTION_MINT" \
    EXT_COLLECTION_MINT="$EXT_COLLECTION_MINT" \
    GATEWAY_PUBKEY="${GATEWAY_PUBKEY:-}" \
    TRUSTED_EXTENSIONS="${TRUSTED_EXTENSIONS:-phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1}" \
    WASM_DIR="$WASM_OUTPUT" \
    nohup ./target/release/title-tee > /tmp/title-tee.log 2>&1 &
  TEE_PID=$!
  echo "$TEE_PID" > "$PID_DIR/tee.pid"
  echo "  TEE起動 (MockRuntime, PID=$TEE_PID)"
  sleep 2
else
  echo "  TEE は既に稼働中 (PID=$TEE_PID)"
fi

# ---------------------------------------------------------------------------
# Step 4: TempStorage + Gateway + PostgreSQL + Indexer の起動
# ---------------------------------------------------------------------------
echo "[Step 4/7] サービス起動..."

# PostgreSQL (Docker)
echo "  PostgreSQL (Docker Compose)..."
docker compose -f deploy/local/docker-compose.yml up -d

# TempStorage
TEMP_STORAGE_PID=$(pgrep -x title-temp-st 2>/dev/null || true)
if [ -z "$TEMP_STORAGE_PID" ]; then
  STORAGE_DIR="/tmp/title-uploads" \
    STORAGE_PORT=3001 \
    nohup ./target/release/title-temp-storage > /tmp/title-temp-storage.log 2>&1 &
  TEMP_STORAGE_PID=$!
  echo "$TEMP_STORAGE_PID" > "$PID_DIR/temp-storage.pid"
  echo "  TempStorage起動 (port 3001, PID=$TEMP_STORAGE_PID)"
  sleep 1
else
  echo "  TempStorage は既に稼働中 (PID=$TEMP_STORAGE_PID)"
fi

# Gateway
GATEWAY_PID=$(pgrep -x title-gateway 2>/dev/null || true)
if [ -z "$GATEWAY_PID" ]; then
  TEE_ENDPOINT="http://localhost:4000" \
    LOCAL_STORAGE_ENDPOINT="http://localhost:3001" \
    GATEWAY_SIGNING_KEY="${GATEWAY_SIGNING_KEY:-}" \
    SOLANA_RPC_URL="$SOLANA_RPC_URL" \
    nohup ./target/release/title-gateway > /tmp/title-gateway.log 2>&1 &
  GATEWAY_PID=$!
  echo "$GATEWAY_PID" > "$PID_DIR/gateway.pid"
  echo "  Gateway起動 (port 3000, PID=$GATEWAY_PID)"
  sleep 1
else
  echo "  Gateway は既に稼働中 (PID=$GATEWAY_PID)"
fi

# Indexer
if [ -d "$PROJECT_ROOT/indexer" ] && command -v node &>/dev/null; then
  INDEXER_PID=$(pgrep -f "node.*indexer" 2>/dev/null || true)
  if [ -z "$INDEXER_PID" ]; then
    (cd "$PROJECT_ROOT/indexer" && npm install --silent 2>/dev/null && npm run build --silent 2>/dev/null) || true
    DATABASE_URL="${DATABASE_URL:-postgres://title:title_dev@localhost:5432/title_indexer}" \
      DAS_ENDPOINTS="${DAS_ENDPOINTS:-$SOLANA_RPC_URL}" \
      COLLECTION_MINTS="${COLLECTION_MINTS:-$CORE_COLLECTION_MINT,$EXT_COLLECTION_MINT}" \
      nohup node "$PROJECT_ROOT/indexer/dist/index.js" > /tmp/title-indexer.log 2>&1 &
    INDEXER_PID=$!
    echo "$INDEXER_PID" > "$PID_DIR/indexer.pid"
    echo "  Indexer起動 (port 5000, PID=$INDEXER_PID)"
  else
    echo "  Indexer は既に稼働中 (PID=$INDEXER_PID)"
  fi
else
  echo "  SKIP: Indexer (Node.js未インストール、またはindexer/が見つかりません)"
fi

echo "  サービスの起動を待機中..."
for i in $(seq 1 15); do
  if curl -sf http://localhost:3001/health > /dev/null 2>&1 && \
     curl -sf -X POST -H "Content-Type: application/json" \
       -d '{"content_size":1,"content_type":"image/jpeg"}' \
       http://localhost:3000/upload-url > /dev/null 2>&1; then
    echo "  TempStorage 応答確認"
    echo "  Gateway 応答確認"
    break
  fi
  sleep 2
done

# ---------------------------------------------------------------------------
# Step 5: TEEノード登録
# ---------------------------------------------------------------------------
echo "[Step 5/7] TEEノード登録..."

./target/release/title-cli register-node \
  --tee-url http://localhost:4000 \
  --gateway-endpoint "http://localhost:3000" \
  2>&1 || true

# ---------------------------------------------------------------------------
# Step 6: Merkle Tree 作成
# ---------------------------------------------------------------------------
echo "[Step 6/7] Merkle Tree 作成..."

./target/release/title-cli create-tree \
  --tee-url http://localhost:4000 \
  --max-depth 14 \
  --max-buffer-size 64 \
  2>&1 || true

# ---------------------------------------------------------------------------
# Step 7: ヘルスチェック
# ---------------------------------------------------------------------------
echo "[Step 7/7] ヘルスチェック..."

# Solana RPC
if curl -sf -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  "$SOLANA_RPC_URL" > /dev/null 2>&1; then
  echo "  OK  Solana RPC"
else
  echo "  NG  Solana RPC ($SOLANA_RPC_URL)"
fi

# TempStorage
if curl -sf http://localhost:3001/health > /dev/null 2>&1; then
  echo "  OK  TempStorage"
else
  echo "  NG  TempStorage"
fi

# Gateway
if curl -sf -X POST -H "Content-Type: application/json" \
  -d '{"content_size":1,"content_type":"image/jpeg"}' \
  http://localhost:3000/upload-url > /dev/null 2>&1; then
  echo "  OK  Gateway"
else
  echo "  NG  Gateway"
fi

# TEE
if curl -sf http://localhost:4000/health > /dev/null 2>&1; then
  echo "  OK  TEE"
else
  echo "  NG  TEE"
fi

# Indexer
if curl -sf http://localhost:5000/health > /dev/null 2>&1; then
  echo "  OK  Indexer"
else
  echo "  NG  Indexer"
fi

echo ""
echo "=== ローカル開発環境 起動完了 ==="
echo ""
echo "  TempStorage: http://localhost:3001"
echo "  Gateway:     http://localhost:3000"
echo "  TEE:         http://localhost:4000"
echo "  Indexer:     http://localhost:5000"
echo "  PostgreSQL:  localhost:5432"
echo ""
echo "  ログ:"
echo "    tail -f /tmp/title-tee.log"
echo "    tail -f /tmp/title-temp-storage.log"
echo "    tail -f /tmp/title-gateway.log"
echo "    tail -f /tmp/title-indexer.log"
echo ""
echo "  停止: ./deploy/local/teardown.sh"
echo ""
