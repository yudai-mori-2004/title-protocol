#!/usr/bin/env bash
# Title Protocol ローカル開発環境初期化スクリプト
#
# docker-compose up 後に実行して、Global Config初期化等を行う。
#
# 前提条件:
#   - docker compose が稼働中
#   - solana CLI がインストール済み
#   - Node.js 20+ がインストール済み
#
# 使い方:
#   docker compose up -d
#   ./scripts/setup-local.sh
#
# Solana RPC を変更する場合:
#   SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=xxx ./scripts/setup-local.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

SOLANA_RPC="${SOLANA_RPC_URL:-https://api.devnet.solana.com}"
GATEWAY_URL="http://localhost:3000"
TEE_URL="http://localhost:4000"
INDEXER_URL="http://localhost:5050"
S3_ENDPOINT="http://localhost:9000"

echo "=== Title Protocol ローカル環境セットアップ ==="
echo "  Solana RPC: $SOLANA_RPC"
echo ""

# ---------------------------------------------------------------------------
# Step 1: 必須ツールの確認
# ---------------------------------------------------------------------------
echo "[Step 1/6] 必須ツールの確認..."

for cmd in solana node docker; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "ERROR: $cmd が見つかりません。インストールしてください。"
    exit 1
  fi
done

echo "  OK"

# ---------------------------------------------------------------------------
# Step 2: Solana キーペアの準備
# ---------------------------------------------------------------------------
echo "[Step 2/6] Solana キーペア準備..."

# Solana CLIの設定
solana config set --url "$SOLANA_RPC" --keypair ~/.config/solana/id.json &>/dev/null 2>&1 || true

# キーペアが存在しなければ作成
if [ ! -f ~/.config/solana/id.json ]; then
  echo "  Solana キーペアを生成中..."
  solana-keygen new --no-bip39-passphrase --silent
fi

# devnetの場合SOLをエアドロップ（失敗しても続行）
echo "  SOLをエアドロップ中..."
solana airdrop 2 --url "$SOLANA_RPC" 2>/dev/null || echo "  (エアドロップスキップ — 手動でSOLを入金してください)"

echo "  OK"

# ---------------------------------------------------------------------------
# Step 3: MinIOバケットの作成
# ---------------------------------------------------------------------------
echo "[Step 3/6] MinIOバケット作成..."

docker compose exec -T minio sh -c '
  mc alias set local http://localhost:9000 minioadmin minioadmin 2>/dev/null
  mc mb local/title-uploads --ignore-existing 2>/dev/null
' 2>/dev/null || echo "  WARNING: MinIOバケット作成に失敗（手動で作成してください）"

echo "  OK"

# ---------------------------------------------------------------------------
# Step 4: C2PAテストフィクスチャの生成
# ---------------------------------------------------------------------------
echo "[Step 4/6] C2PAテストフィクスチャ生成..."

FIXTURE_DIR="$PROJECT_ROOT/tests/e2e/fixtures"
mkdir -p "$FIXTURE_DIR"

if [ ! -f "$FIXTURE_DIR/signed.jpg" ]; then
  cargo run --example gen_fixture -p title-core -- "$FIXTURE_DIR" 2>/dev/null \
    && echo "  OK" \
    || echo "  SKIP（cargo example未ビルド。手動: cargo run --example gen_fixture -p title-core -- $FIXTURE_DIR）"
else
  echo "  OK（既存のフィクスチャを使用）"
fi

# ---------------------------------------------------------------------------
# Step 5: Global Config初期化 + TEE /create-tree
# ---------------------------------------------------------------------------
echo "[Step 5/6] Global Config初期化 + Merkle Tree作成..."

cd "$PROJECT_ROOT"
node "$SCRIPT_DIR/init-config.mjs" \
  --rpc "$SOLANA_RPC" \
  --gateway "$GATEWAY_URL" \
  --tee "$TEE_URL"

echo "  OK"

# ---------------------------------------------------------------------------
# Step 6: サービス動作確認
# ---------------------------------------------------------------------------
echo "[Step 6/6] サービス動作確認..."

check_service() {
  local name="$1"
  local url="$2"
  if curl -sf "$url" >/dev/null 2>&1; then
    echo "  ✓ $name"
  else
    echo "  ✗ $name ($url)"
  fi
}

# Solana RPCはJSON-RPCなのでPOSTでチェック
if curl -sf -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  "$SOLANA_RPC" >/dev/null 2>&1; then
  echo "  ✓ Solana RPC"
else
  echo "  ✗ Solana RPC ($SOLANA_RPC)"
fi

check_service "MinIO" "$S3_ENDPOINT/minio/health/live"
check_service "Gateway" "$GATEWAY_URL/.well-known/title-node-info"
check_service "TEE Mock" "$TEE_URL/health" || true
check_service "Indexer" "$INDEXER_URL/health"

echo ""
echo "=== セットアップ完了 ==="
echo ""
echo "E2Eテストの実行:"
echo "  cd tests/e2e && npm run build && npm test"
