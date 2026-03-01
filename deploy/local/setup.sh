#!/usr/bin/env bash
# Title Protocol ローカル開発 起動スクリプト
#
# すべてのプロセスをホスト上で直接起動する。
#
# 前提:
#   - network.json が存在する（title-cli init-global で事前に作成済み）
#   - .env が設定済み（SOLANA_RPC_URL のみ必須）
#   - Rust, Solana CLI, Docker, Node.js がインストール済み
#
# やること:
#   0. 前提条件チェック + .env + network.json の読み込み・検証
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

# ステータス表示用ヘルパー
ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }
warn() { echo "  ! $1"; }

echo "=== Title Protocol ローカル開発 起動 ==="

# ---------------------------------------------------------------------------
# Step 0: 前提条件チェック + 設定ファイル読み込み
# ---------------------------------------------------------------------------
echo ""
echo "[Step 0/7] 前提条件チェック..."

MISSING_DEPS=()
command -v cargo    &>/dev/null || MISSING_DEPS+=("Rust (https://rustup.rs/)")
command -v solana   &>/dev/null || MISSING_DEPS+=("Solana CLI (https://docs.solana.com/cli/install-solana-cli-tools)")
command -v docker   &>/dev/null || MISSING_DEPS+=("Docker (https://docs.docker.com/get-docker/)")
command -v python3  &>/dev/null || MISSING_DEPS+=("python3 (macOS/Linux に標準搭載)")

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
  echo "ERROR: 以下のツールがインストールされていません:"
  for dep in "${MISSING_DEPS[@]}"; do
    echo "  - $dep"
  done
  exit 1
fi

# Docker デーモンの起動チェック
if ! docker info &>/dev/null 2>&1; then
  echo "ERROR: Docker デーモンが起動していません。"
  echo "  Docker Desktop を起動してから再実行してください。"
  exit 1
fi

# Node.js (Indexer用、オプション)
HAS_NODE=false
if command -v node &>/dev/null; then
  HAS_NODE=true
  ok "Node.js $(node --version)"
else
  warn "Node.js が見つかりません。Indexer はスキップされます。"
fi

ok "Rust $(rustc --version | awk '{print $2}')"
ok "Solana CLI $(solana --version 2>&1 | awk '{print $2}')"
ok "Docker $(docker --version | awk '{print $3}' | tr -d ',')"

# wasm32-unknown-unknown ターゲット
if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
  warn "wasm32-unknown-unknown ターゲットが未インストール。自動追加します..."
  rustup target add wasm32-unknown-unknown
fi
ok "wasm32-unknown-unknown ターゲット"

# .env
echo ""
echo "  設定ファイルの読み込み..."

if [ ! -f .env ]; then
  echo "ERROR: .env が見つかりません。"
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
if [ -z "${SOLANA_RPC_URL:-}" ]; then
  echo "ERROR: SOLANA_RPC_URL が .env に設定されていません。"
  exit 1
fi

ok ".env: OK"

# GATEWAY_SIGNING_KEY の自動生成
# Gateway と register-node が同じ鍵を使う必要があるため、ここで生成して両方に渡す
if [ -z "${GATEWAY_SIGNING_KEY:-}" ]; then
  GATEWAY_SIGNING_KEY=$(openssl rand -hex 32)
  warn "GATEWAY_SIGNING_KEY を自動生成しました（開発環境用）"
fi
export GATEWAY_SIGNING_KEY

# keys/ ディレクトリ（キーペア管理の一元化）
KEYS_DIR="$PROJECT_ROOT/keys"
mkdir -p "$KEYS_DIR"

# Authority keypair の存在チェック（レガシーパスからの自動マイグレーション）
AUTHORITY_KEY_PATH="$KEYS_DIR/authority.json"
LEGACY_AUTHORITY="$PROJECT_ROOT/programs/title-config/keys/authority.json"
if [ ! -f "$AUTHORITY_KEY_PATH" ] && [ -f "$LEGACY_AUTHORITY" ]; then
  warn "レガシーパスから authority.json を keys/ に移行します..."
  cp "$LEGACY_AUTHORITY" "$AUTHORITY_KEY_PATH"
  ok "authority.json を keys/ に移行"
fi

if [ -f "$AUTHORITY_KEY_PATH" ]; then
  ok "Authority keypair (keys/authority.json): 検出 → 自動署名モード"
  AUTO_SIGN=true
else
  warn "Authority keypair: なし → DAO承認モード"
  AUTO_SIGN=false
fi

# Operator keypair の確認（ノード運営者の資金元ウォレット）
OPERATOR_KEY_PATH="$KEYS_DIR/operator.json"
if [ ! -f "$OPERATOR_KEY_PATH" ]; then
  if [ -f "$HOME/.config/solana/id.json" ]; then
    warn "keys/operator.json が見つかりません。~/.config/solana/id.json からコピーします..."
    cp "$HOME/.config/solana/id.json" "$OPERATOR_KEY_PATH"
  else
    warn "オペレーターキーペアが見つかりません。自動作成します..."
    solana-keygen new --no-bip39-passphrase -o "$OPERATOR_KEY_PATH"
  fi
fi
WALLET_PUBKEY=$(solana-keygen pubkey "$OPERATOR_KEY_PATH")
ok "オペレーターウォレット (keys/operator.json): $WALLET_PUBKEY"
solana config set --url "$SOLANA_RPC_URL" > /dev/null 2>&1 || true

# SOL残高チェック（ノード登録 + Merkle Tree 作成に ~0.6 SOL 必要）
REQUIRED_SOL="0.6"
BALANCE=$(solana balance "$WALLET_PUBKEY" --url "$SOLANA_RPC_URL" 2>/dev/null | awk '{print $1}')
if [ -n "$BALANCE" ] && python3 -c "exit(0 if float('$BALANCE') >= $REQUIRED_SOL else 1)" 2>/dev/null; then
  ok "SOL残高: ${BALANCE} SOL"
else
  echo ""
  echo "  ⚠ SOL残高が不足しています（現在: ${BALANCE:-0} SOL、必要: ~${REQUIRED_SOL} SOL）"
  echo ""
  echo "  以下のアドレスに ${REQUIRED_SOL} SOL 以上を送金してください:"
  echo "    $WALLET_PUBKEY"
  echo ""
  echo "  devnet の場合: https://faucet.solana.com で取得できます"
  echo ""
  read -rp "  送金完了後、Enter を押してください... "
  BALANCE=$(solana balance "$WALLET_PUBKEY" --url "$SOLANA_RPC_URL" 2>/dev/null | awk '{print $1}')
  ok "SOL残高: ${BALANCE:-?} SOL"
fi

# ---------------------------------------------------------------------------
# Step 1: WASMモジュールのビルド
# ---------------------------------------------------------------------------
echo ""
echo "  NOTE: 初回ビルドには10〜20分かかる場合があります（2回目以降はキャッシュが効きます）"
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
ok "WASMモジュール → $WASM_OUTPUT/"

# ---------------------------------------------------------------------------
# Step 2: ホスト側バイナリのビルド
# ---------------------------------------------------------------------------
echo ""
echo "[Step 2/7] ホスト側バイナリのビルド..."

echo "  title-tee をビルド中..."
cargo build --release --bin title-tee

echo "  title-cli をビルド中..."
cargo build --release --bin title-cli

echo "  title-temp-storage をビルド中..."
cargo build --release --manifest-path deploy/local/temp-storage/Cargo.toml

echo "  title-gateway をビルド中 (vendor-local)..."
cargo build --release -p title-gateway --no-default-features --features vendor-local

ok "ビルド完了"

# ---------------------------------------------------------------------------
# Step 3: TEE の起動
# ---------------------------------------------------------------------------
echo ""
echo "[Step 3/7] TEE の起動..."

TEE_PID=$(pgrep -f title-tee 2>/dev/null | head -1 || true)
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
  ok "TEE起動 (MockRuntime, PID=$TEE_PID)"
  sleep 2
else
  ok "TEE は既に稼働中 (PID=$TEE_PID)"
fi

# ---------------------------------------------------------------------------
# Step 4: TempStorage + Gateway + PostgreSQL + Indexer の起動
# ---------------------------------------------------------------------------
echo ""
echo "[Step 4/7] サービス起動..."

# PostgreSQL (Docker)
echo "  PostgreSQL (Docker Compose)..."
docker compose -f deploy/local/docker-compose.yml up -d

# TempStorage
TEMP_STORAGE_PID=$(pgrep -f title-temp-storage 2>/dev/null | head -1 || true)
if [ -z "$TEMP_STORAGE_PID" ]; then
  STORAGE_DIR="/tmp/title-uploads" \
    STORAGE_PORT=3001 \
    nohup ./deploy/local/temp-storage/target/release/title-temp-storage > /tmp/title-temp-storage.log 2>&1 &
  TEMP_STORAGE_PID=$!
  echo "$TEMP_STORAGE_PID" > "$PID_DIR/temp-storage.pid"
  ok "TempStorage起動 (port 3001, PID=$TEMP_STORAGE_PID)"
  sleep 1
else
  ok "TempStorage は既に稼働中 (PID=$TEMP_STORAGE_PID)"
fi

# Gateway
GATEWAY_PID=$(pgrep -f title-gateway 2>/dev/null | head -1 || true)
if [ -z "$GATEWAY_PID" ]; then
  TEE_ENDPOINT="http://localhost:4000" \
    LOCAL_STORAGE_ENDPOINT="http://localhost:3001" \
    GATEWAY_SIGNING_KEY="$GATEWAY_SIGNING_KEY" \
    SOLANA_RPC_URL="$SOLANA_RPC_URL" \
    GLOBAL_CONFIG_PDA="$GLOBAL_CONFIG_PDA" \
    nohup ./target/release/title-gateway > /tmp/title-gateway.log 2>&1 &
  GATEWAY_PID=$!
  echo "$GATEWAY_PID" > "$PID_DIR/gateway.pid"
  ok "Gateway起動 (port 3000, PID=$GATEWAY_PID)"
  sleep 1
else
  ok "Gateway は既に稼働中 (PID=$GATEWAY_PID)"
fi

# Indexer
INDEXER_PORT="${WEBHOOK_PORT:-5001}"
if [ "$HAS_NODE" = true ] && [ -d "$PROJECT_ROOT/indexer" ]; then
  INDEXER_PID=$(pgrep -f "node.*indexer" 2>/dev/null || true)
  if [ -z "$INDEXER_PID" ]; then
    (cd "$PROJECT_ROOT/indexer" && npm install --silent && npm run build --silent) || warn "Indexer のビルドに失敗しました（npm install または npm run build）"
    DATABASE_URL="${DATABASE_URL:-postgres://title:title_dev@localhost:5432/title_indexer}" \
      DAS_ENDPOINTS="${DAS_ENDPOINTS:-$SOLANA_RPC_URL}" \
      COLLECTION_MINTS="${COLLECTION_MINTS:-$CORE_COLLECTION_MINT,$EXT_COLLECTION_MINT}" \
      WEBHOOK_PORT="$INDEXER_PORT" \
      nohup node "$PROJECT_ROOT/indexer/dist/index.js" > /tmp/title-indexer.log 2>&1 &
    INDEXER_PID=$!
    echo "$INDEXER_PID" > "$PID_DIR/indexer.pid"
    ok "Indexer起動 (port $INDEXER_PORT, PID=$INDEXER_PID)"
  else
    ok "Indexer は既に稼働中 (PID=$INDEXER_PID)"
  fi
else
  warn "Indexer をスキップ (Node.js未インストール)"
fi

echo ""
echo "  サービスの起動を待機中..."
GATEWAY_READY=false
for i in $(seq 1 15); do
  if curl -sf http://localhost:3001/health > /dev/null 2>&1 && \
     curl -sf http://localhost:3000/health > /dev/null 2>&1; then
    ok "TempStorage 応答確認"
    ok "Gateway 応答確認"
    GATEWAY_READY=true
    break
  fi
  sleep 2
done

if [ "$GATEWAY_READY" = false ]; then
  fail "Gateway / TempStorage が応答しません"
  echo "  ログを確認してください:"
  echo "    tail -f /tmp/title-gateway.log"
  echo "    tail -f /tmp/title-temp-storage.log"
  exit 1
fi

# ---------------------------------------------------------------------------
# Step 5: TEEノード登録
# ---------------------------------------------------------------------------
echo ""
echo "[Step 5/7] TEEノード登録..."

REGISTER_OUTPUT=$(./target/release/title-cli register-node \
  --tee-url http://localhost:4000 \
  --gateway-endpoint "http://localhost:3000" \
  2>&1) && REGISTER_OK=true || REGISTER_OK=false

echo "$REGISTER_OUTPUT" | sed 's/^/  /'

if [ "$REGISTER_OK" = true ]; then
  ok "TEEノード登録 OK"
else
  fail "TEEノード登録に失敗しました"
  echo "  確認事項:"
  echo "    - SOL残高: solana balance --url $SOLANA_RPC_URL"
  echo "    - TEEログ: tail -20 /tmp/title-tee.log"
  echo "  手動で再実行: ./target/release/title-cli register-node --tee-url http://localhost:4000 --gateway-endpoint http://localhost:3000"
fi

# ---------------------------------------------------------------------------
# Step 6: Merkle Tree 作成
# ---------------------------------------------------------------------------
echo ""
echo "[Step 6/7] Merkle Tree 作成..."

TREE_OUTPUT=$(./target/release/title-cli create-tree \
  --tee-url http://localhost:4000 \
  --max-depth 14 \
  --max-buffer-size 64 \
  2>&1) && TREE_OK=true || TREE_OK=false

echo "$TREE_OUTPUT" | sed 's/^/  /'

if [ "$TREE_OK" = true ]; then
  ok "Merkle Tree OK"
else
  fail "Merkle Tree 作成に失敗しました"
  echo "  確認事項:"
  echo "    - SOL残高: solana balance --url $SOLANA_RPC_URL"
  echo "    - TEEログ: tail -20 /tmp/title-tee.log"
  echo "  手動で再実行: ./target/release/title-cli create-tree --tee-url http://localhost:4000 --max-depth 14 --max-buffer-size 64"
fi

# ---------------------------------------------------------------------------
# Step 7: ヘルスチェック
# ---------------------------------------------------------------------------
echo ""
echo "[Step 7/7] ヘルスチェック..."

ALL_OK=true

# Solana RPC
if curl -sf -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  "$SOLANA_RPC_URL" > /dev/null 2>&1; then
  ok "Solana RPC"
else
  fail "Solana RPC ($SOLANA_RPC_URL)"
  ALL_OK=false
fi

# TempStorage
if curl -sf http://localhost:3001/health > /dev/null 2>&1; then
  ok "TempStorage (:3001)"
else
  fail "TempStorage"
  ALL_OK=false
fi

# Gateway
if curl -sf http://localhost:3000/health > /dev/null 2>&1; then
  ok "Gateway (:3000)"
else
  fail "Gateway"
  ALL_OK=false
fi

# TEE
if curl -sf http://localhost:4000/health > /dev/null 2>&1; then
  ok "TEE (:4000)"
else
  fail "TEE"
  ALL_OK=false
fi

# Indexer
if curl -sf http://localhost:${INDEXER_PORT}/health > /dev/null 2>&1; then
  ok "Indexer (:${INDEXER_PORT})"
else
  if [ "$HAS_NODE" = true ]; then
    fail "Indexer"
    ALL_OK=false
  else
    warn "Indexer (スキップ済み)"
  fi
fi

echo ""
if [ "$ALL_OK" = true ]; then
  echo "=== ローカル開発環境 起動完了（全サービス正常） ==="
else
  echo "=== ローカル開発環境 起動完了（一部サービスに問題あり） ==="
fi
echo ""
echo "  TempStorage: http://localhost:3001"
echo "  Gateway:     http://localhost:3000"
echo "  TEE:         http://localhost:4000"
echo "  Indexer:     http://localhost:${INDEXER_PORT}"
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
