#!/usr/bin/env bash
# Title Protocol EC2 デプロイスクリプト
#
# EC2インスタンス上で実行し、以下を行う:
#   1. .env の読み込み・検証
#   2. WASMモジュールのビルド（またはコピー）
#   3. Enclave イメージのビルド + 起動
#   4. Proxy の起動
#   5. Docker Compose (Gateway + PostgreSQL + Indexer) の起動
#   6. S3バケットの確認
#   7. Global Config 初期化 + Merkle Tree 作成
#   8. ヘルスチェック
#
# 前提:
#   - user-data.sh による初期セットアップ完了
#   - .env が設定済み
#   - リポジトリがクローン済み
#
# 使い方:
#   cd ~/title-protocol
#   ./deploy/aws/setup-ec2.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_ROOT"

echo "=== Title Protocol Devnet デプロイ ==="

# ---------------------------------------------------------------------------
# Step 0: .env 読み込み・検証
# ---------------------------------------------------------------------------
echo "[Step 0/8] .env の読み込み..."

if [ ! -f .env ]; then
  echo "ERROR: .env が見つかりません。.env.example をコピーして設定してください。"
  echo "  cp .env.example .env && vim .env"
  exit 1
fi

set -a
source .env
set +a

# 必須変数の検証
REQUIRED_VARS=(
  SOLANA_RPC_URL
  GATEWAY_SIGNING_KEY
  S3_ENDPOINT
  S3_ACCESS_KEY
  S3_SECRET_KEY
  DB_PASSWORD
)
# COLLECTION_MINT, GATEWAY_PUBKEY は init-config.mjs 実行後に設定するため、初回は不要

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

echo "  OK（必須変数チェック通過）"

# ---------------------------------------------------------------------------
# Step 1: WASMモジュールのビルド
# ---------------------------------------------------------------------------
echo "[Step 1/8] WASMモジュールのビルド..."

WASM_OUTPUT="$PROJECT_ROOT/wasm-modules"
mkdir -p "$WASM_OUTPUT"

WASM_TARGETS=(phash-v1 hardware-google c2pa-training-v1 c2pa-license-v1)

export OPENSSL_NO_VENDOR=1

if command -v cargo &>/dev/null && rustup target list --installed | grep -q wasm32-unknown-unknown; then
  for module in "${WASM_TARGETS[@]}"; do
    echo "  ビルド中: $module ..."
    (cd "wasm/$module" && cargo build --target wasm32-unknown-unknown --release)
    cp "wasm/$module/target/wasm32-unknown-unknown/release/${module//-/_}.wasm" "$WASM_OUTPUT/$module.wasm"
  done
  echo "  WASMモジュール → $WASM_OUTPUT/"
else
  echo "  SKIP: Rust/wasm32ターゲットが未インストール。"
  echo "  事前にビルド済みの .wasm を $WASM_OUTPUT/ に配置してください。"
  # 確認
  for module in "${WASM_TARGETS[@]}"; do
    if [ ! -f "$WASM_OUTPUT/$module.wasm" ]; then
      echo "  WARNING: $WASM_OUTPUT/$module.wasm が見つかりません"
    fi
  done
fi

# ---------------------------------------------------------------------------
# Step 2: Enclave イメージのビルド
# ---------------------------------------------------------------------------
echo "[Step 2/8] Enclave イメージのビルド..."

EIF_PATH="$PROJECT_ROOT/title-tee.eif"

if command -v nitro-cli &>/dev/null; then
  # TEE Docker イメージのビルド
  docker build -t title-tee-enclave -f deploy/aws/docker/tee.Dockerfile .

  # EIF生成
  nitro-cli build-enclave \
    --docker-uri title-tee-enclave:latest \
    --output-file "$EIF_PATH" 2>&1 | tee /tmp/enclave-build.log

  echo "  EIF: $EIF_PATH"

  # PCR値の表示
  echo "  PCR測定値:"
  nitro-cli describe-eif --eif-path "$EIF_PATH" | \
    python3 -c "
import sys, json
data = json.load(sys.stdin)
measurements = data.get('Measurements', {})
for key in ['PCR0', 'PCR1', 'PCR2']:
    val = measurements.get(key, 'N/A')
    print(f'    {key}: {val}')
" 2>/dev/null || echo "    (PCR表示にはpython3が必要)"
else
  echo "  SKIP: nitro-cli が未インストール。Enclaveなしで続行（MockRuntimeを使用）。"
fi

# ---------------------------------------------------------------------------
# Step 3: Enclave の起動
# ---------------------------------------------------------------------------
echo "[Step 3/8] Enclave の起動..."

ENCLAVE_CPU="${ENCLAVE_CPU_COUNT:-2}"
ENCLAVE_MEM="${ENCLAVE_MEMORY_MIB:-512}"

if command -v nitro-cli &>/dev/null && [ -f "$EIF_PATH" ]; then
  # 既存Enclaveの停止（存在する場合）
  EXISTING=$(nitro-cli describe-enclaves | python3 -c "
import sys, json
data = json.load(sys.stdin)
for e in data:
    if e.get('State') == 'RUNNING':
        print(e['EnclaveID'])
" 2>/dev/null || true)

  if [ -n "$EXISTING" ]; then
    echo "  既存Enclaveを停止: $EXISTING"
    nitro-cli terminate-enclave --enclave-id "$EXISTING"
    sleep 2
  fi

  # Enclave起動
  ENCLAVE_OUTPUT=$(nitro-cli run-enclave \
    --eif-path "$EIF_PATH" \
    --cpu-count "$ENCLAVE_CPU" \
    --memory "$ENCLAVE_MEM")

  echo "$ENCLAVE_OUTPUT"

  # Enclave CIDを取得
  ENCLAVE_CID=$(echo "$ENCLAVE_OUTPUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['EnclaveCID'])")
  echo "  Enclave起動完了 (CPU=$ENCLAVE_CPU, Memory=${ENCLAVE_MEM}MiB, CID=$ENCLAVE_CID)"

  # インバウンドブリッジ: Gateway (TCP:4000) → Enclave (vsock:CID:4000)
  # Enclave内のsocatがvsock:4000→TCP:localhost:4000に中継し、title-teeに到達する
  pkill -f "socat TCP-LISTEN:4000" 2>/dev/null || true
  sleep 1
  socat TCP-LISTEN:4000,fork,reuseaddr VSOCK-CONNECT:"$ENCLAVE_CID":4000 &
  echo "  インバウンドブリッジ起動 (TCP:4000 → vsock:$ENCLAVE_CID:4000)"
else
  # MockRuntime: TEEバイナリを直接起動
  echo "  Enclaveなし。TEEをMockRuntimeで直接起動します。"
  # pgrep でバイナリ名を正確に検索（nitro-cli の引数に含まれる "title-tee.eif" を除外）
  TEE_PID=$(pgrep -x title-tee 2>/dev/null || true)
  if [ -z "$TEE_PID" ]; then
    if [ -f "target/release/title-tee" ]; then
      MOCK_MODE=true TEE_RUNTIME=mock PROXY_ADDR=direct \
        SOLANA_RPC_URL="$SOLANA_RPC_URL" \
        COLLECTION_MINT="${COLLECTION_MINT:-}" \
        GATEWAY_PUBKEY="${GATEWAY_PUBKEY:-}" \
        TRUSTED_EXTENSIONS="${TRUSTED_EXTENSIONS:-phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1}" \
        WASM_DIR="$WASM_OUTPUT" \
        nohup ./target/release/title-tee > /tmp/title-tee.log 2>&1 &
      echo "  TEE起動 (MockRuntime, PID=$!)"
      sleep 2
    else
      echo "  ERROR: target/release/title-tee が見つかりません。"
      echo "    OPENSSL_NO_VENDOR=1 cargo build --release --bin title-tee"
      exit 1
    fi
  else
    echo "  TEE は既に稼働中"
  fi
fi

# ---------------------------------------------------------------------------
# Step 4: Proxy の起動
# ---------------------------------------------------------------------------
echo "[Step 4/8] Proxy の起動..."

if command -v nitro-cli &>/dev/null && [ -f "$EIF_PATH" ]; then
  # 本番: vsock経由のProxy
  if ! pgrep -f title-proxy &>/dev/null; then
    if [ -f "target/release/title-proxy" ]; then
      nohup ./target/release/title-proxy > /var/log/title-proxy.log 2>&1 &
      echo "  Proxy起動 (vsock mode, PID=$!)"
    else
      echo "  Proxyバイナリが見つかりません。ビルドしてください:"
      echo "    cargo build --release --bin title-proxy"
    fi
  else
    echo "  Proxy は既に稼働中"
  fi
else
  echo "  SKIP: MockRuntimeモードではProxyは不要"
fi

# ---------------------------------------------------------------------------
# Step 5: Docker Compose (Gateway + PostgreSQL + Indexer)
# ---------------------------------------------------------------------------
echo "[Step 5/8] Docker Compose 起動..."

# 本番compose: PostgreSQL + Gateway + Indexer（TEEは別プロセスで起動済み）
docker compose -f deploy/aws/docker-compose.production.yml up -d --build

echo "  Docker Compose 起動完了"

# サービスの起動待ち
echo "  サービスの起動を待機中..."
for i in $(seq 1 30); do
  if curl -sf http://localhost:3000/.well-known/title-node-info > /dev/null 2>&1; then
    echo "  Gateway 応答確認"
    break
  fi
  sleep 2
done

# ---------------------------------------------------------------------------
# Step 6: S3バケットの確認
# ---------------------------------------------------------------------------
echo "[Step 6/8] S3ストレージの確認..."

BUCKET_NAME="${S3_BUCKET:-title-uploads}"
if echo "$S3_ENDPOINT" | grep -q "s3.amazonaws.com\|s3\..*\.amazonaws\.com"; then
  # AWS S3: バケット存在確認
  echo "  S3バケット: $BUCKET_NAME (endpoint: $S3_ENDPOINT)"
  if aws s3 ls "s3://$BUCKET_NAME" > /dev/null 2>&1; then
    echo "  S3バケット確認OK"
  else
    echo "  WARNING: S3バケットにアクセスできません"
    echo "    確認事項:"
    echo "      - S3_BUCKET=$BUCKET_NAME がTerraformで作成したバケット名と一致しているか"
    echo "      - EC2のIAMロールにS3アクセス権限があるか"
    echo "      - aws s3 ls s3://$BUCKET_NAME を手動で試してみてください"
  fi
else
  # MinIO/ローカル: docker composeのMinIOを使用
  echo "  S3互換エンドポイント: $S3_ENDPOINT"
  docker compose exec -T minio sh -c '
    mc alias set local http://localhost:9000 '"$S3_ACCESS_KEY"' '"$S3_SECRET_KEY"' 2>/dev/null
    mc mb local/title-uploads --ignore-existing 2>/dev/null
  ' 2>/dev/null && echo "  バケット確認OK" || echo "  WARNING: バケット作成失敗"
fi

# ---------------------------------------------------------------------------
# Step 7: Global Config 初期化 + Merkle Tree 作成
# ---------------------------------------------------------------------------
echo "[Step 7/8] Global Config 初期化..."

# init-config.mjs の依存インストール
if [ ! -d "$PROJECT_ROOT/scripts/node_modules" ]; then
  echo "  npm install (scripts/)..."
  (cd "$PROJECT_ROOT/scripts" && npm install --silent)
fi

# TEEエンドポイントの決定
if command -v nitro-cli &>/dev/null && [ -f "$EIF_PATH" ]; then
  TEE_URL="http://localhost:4000"  # Proxy経由
else
  TEE_URL="${TEE_ENDPOINT:-http://localhost:4000}"
fi

node scripts/init-config.mjs \
  --rpc "$SOLANA_RPC_URL" \
  --gateway "http://localhost:3000" \
  --tee "$TEE_URL"

echo "  OK"

# ---------------------------------------------------------------------------
# Step 8: ヘルスチェック
# ---------------------------------------------------------------------------
echo "[Step 8/8] ヘルスチェック..."

check_service() {
  local name="$1"
  local url="$2"
  if curl -sf "$url" > /dev/null 2>&1; then
    echo "  OK  $name"
  else
    echo "  NG  $name ($url)"
  fi
}

# Solana RPC
if curl -sf -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  "$SOLANA_RPC_URL" > /dev/null 2>&1; then
  echo "  OK  Solana RPC"
else
  echo "  NG  Solana RPC ($SOLANA_RPC_URL)"
fi

check_service "Gateway" "http://localhost:3000/.well-known/title-node-info"
check_service "TEE" "$TEE_URL/health"
check_service "Indexer" "http://localhost:5000/health"

# Gateway ノード情報の表示
echo ""
echo "--- ノード情報 ---"
curl -sf http://localhost:3000/.well-known/title-node-info 2>/dev/null | python3 -m json.tool 2>/dev/null || true

echo ""
echo "=== デプロイ完了 ==="
echo ""
echo "Gateway API: http://$(curl -sf http://169.254.169.254/latest/meta-data/public-ipv4 2>/dev/null || echo 'localhost'):3000"
echo ""
echo "E2Eテスト:"
echo "  GATEWAY_URL=http://<public-ip>:3000 npm test --prefix tests/e2e"
