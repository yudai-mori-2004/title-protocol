#!/usr/bin/env bash
# Title Protocol ノード起動スクリプト
#
# EC2インスタンス上で実行し、1つのTEEノードを起動する。
# 冪等: 何度実行しても同じ結果になる。各インスタンスに1つのTEE。
#
# 前提:
#   - network.json が存在する（title-cli init-global で事前に作成済み）
#   - .env が設定済み
#   - user-data.sh による初期セットアップ完了
#
# やること:
#   1. .env + network.json の読み込み・検証
#   2. WASMモジュールのビルド
#   3. ホスト側バイナリのビルド
#   4. Enclave イメージのビルド + 起動（またはMockRuntime直接起動）
#   5. Proxy の起動（Enclaveモードのみ）
#   6. Docker Compose (Gateway) の起動
#   7. S3バケットの確認
#   8. TEEノード登録 (/register-node → DAO署名)
#   9. Merkle Tree 作成 (/create-tree)
#  10. ヘルスチェック
#
# Authority keypair が keys/authority.json に存在する場合:
#   → register-node TX を自動で共同署名しブロードキャストする（devnet向け）
# 存在しない場合:
#   → 部分署名済みTXをファイルに保存し、DAO承認待ちとする（mainnet向け）
#
# 使い方:
#   cd ~/title-protocol
#   ./deploy/aws/setup-ec2.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_ROOT"

# ---------------------------------------------------------------------------
# Docker グループチェック
# ---------------------------------------------------------------------------
if ! groups | grep -q docker; then
  echo "docker グループ未反映。sg docker で再実行します..."
  exec sg docker "$0"
fi

export PATH="$HOME/.cargo/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"

echo "=== Title Protocol ノード起動 ==="

# ---------------------------------------------------------------------------
# Step 0: .env + network.json 読み込み・検証
# ---------------------------------------------------------------------------
echo "[Step 0/10] 設定ファイルの読み込み..."

# .env
if [ ! -f .env ]; then
  echo "ERROR: .env が見つかりません。.env.example をコピーして設定してください。"
  echo "  cp .env.example .env && vim .env"
  exit 1
fi

set -a
source .env
set +a

# GATEWAY_SIGNING_KEY の自動生成
# Gateway と register-node が同じ鍵を使う必要があるため、ここで生成して両方に渡す
if [ -z "${GATEWAY_SIGNING_KEY:-}" ]; then
  GATEWAY_SIGNING_KEY=$(openssl rand -hex 32)
  echo "  GATEWAY_SIGNING_KEY を自動生成しました"
fi
export GATEWAY_SIGNING_KEY

# network.json
NETWORK_JSON="$PROJECT_ROOT/network.json"
if [ ! -f "$NETWORK_JSON" ]; then
  echo "ERROR: network.json が見つかりません。"
  echo "  先に title-cli init-global でGlobalConfigを作成してください:"
  echo "    cargo run --release --bin title-cli -- init-global"
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

# .env から CORE_COLLECTION_MINT/EXT_COLLECTION_MINT が未設定なら network.json の値を使う
CORE_COLLECTION_MINT="${CORE_COLLECTION_MINT:-$CORE_COLLECTION_MINT_NET}"
EXT_COLLECTION_MINT="${EXT_COLLECTION_MINT:-$EXT_COLLECTION_MINT_NET}"

# 必須変数の検証
REQUIRED_VARS=(
  SOLANA_RPC_URL
  S3_ENDPOINT
  S3_BUCKET
  S3_ACCESS_KEY
  S3_SECRET_KEY
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

# keys/ ディレクトリ（キーペア管理の一元化）
KEYS_DIR="$PROJECT_ROOT/keys"
mkdir -p "$KEYS_DIR"

# Authority keypair の存在チェック
AUTHORITY_KEY_PATH="$KEYS_DIR/authority.json"
if [ -f "$AUTHORITY_KEY_PATH" ]; then
  echo "  Authority keypair (keys/authority.json): 検出 → 自動署名モード (devnet)"
  AUTO_SIGN=true
else
  echo "  Authority keypair: なし → DAO承認モード (mainnet)"
  AUTO_SIGN=false
fi

# Operator keypair の確認（ノード運営者の資金元ウォレット）
OPERATOR_KEY_PATH="$KEYS_DIR/operator.json"
if [ ! -f "$OPERATOR_KEY_PATH" ]; then
  if [ -f "$HOME/.config/solana/id.json" ]; then
    echo "  keys/operator.json が見つかりません。~/.config/solana/id.json からコピーします..."
    cp "$HOME/.config/solana/id.json" "$OPERATOR_KEY_PATH"
  else
    echo "  オペレーターキーペアが見つかりません。自動作成します..."
    solana-keygen new --no-bip39-passphrase -o "$OPERATOR_KEY_PATH"
  fi
fi
WALLET_PUBKEY=$(solana-keygen pubkey "$OPERATOR_KEY_PATH")
echo "  オペレーターウォレット (keys/operator.json): $WALLET_PUBKEY"
solana config set --url "$SOLANA_RPC_URL" > /dev/null 2>&1 || true

# SOL残高チェック（ノード登録 + Merkle Tree 作成に ~0.6 SOL 必要）
REQUIRED_SOL="0.6"
BALANCE=$(solana balance "$WALLET_PUBKEY" --url "$SOLANA_RPC_URL" 2>/dev/null | awk '{print $1}')
if [ -n "$BALANCE" ] && python3 -c "exit(0 if float('$BALANCE') >= $REQUIRED_SOL else 1)" 2>/dev/null; then
  echo "  SOL残高: ${BALANCE} SOL"
else
  echo ""
  echo "  WARNING: SOL残高が不足しています（現在: ${BALANCE:-0} SOL、必要: ~${REQUIRED_SOL} SOL）"
  echo ""
  echo "  以下のアドレスに ${REQUIRED_SOL} SOL 以上を送金してください:"
  echo "    $WALLET_PUBKEY"
  echo ""
  echo "  devnet の場合:"
  echo "    solana airdrop 2 $WALLET_PUBKEY --url devnet"
  echo "    または https://faucet.solana.com で取得"
  echo "    (EC2からのエアドロップはrate limitされることがあります。ローカルから送金も可能:"
  echo "     solana transfer $WALLET_PUBKEY 2 --url devnet)"
  echo ""
  read -rp "  送金完了後、Enter を押してください... "
  BALANCE=$(solana balance "$WALLET_PUBKEY" --url "$SOLANA_RPC_URL" 2>/dev/null | awk '{print $1}')
  echo "  SOL残高: ${BALANCE:-?} SOL"
fi

# ---------------------------------------------------------------------------
# Step 1: WASMモジュールのビルド
# ---------------------------------------------------------------------------
echo ""
echo "[Step 1/10] WASMモジュールのビルド..."

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
  for module in "${WASM_TARGETS[@]}"; do
    if [ ! -f "$WASM_OUTPUT/$module.wasm" ]; then
      echo "  WARNING: $WASM_OUTPUT/$module.wasm が見つかりません"
    fi
  done
fi

# ---------------------------------------------------------------------------
# Step 2: ホスト側バイナリのビルド
# ---------------------------------------------------------------------------
echo ""
echo "[Step 2/10] ホスト側バイナリのビルド..."

if command -v cargo &>/dev/null; then
  if command -v nitro-cli &>/dev/null; then
    echo "  title-proxy をビルド中..."
    cargo build --release --bin title-proxy
  else
    echo "  title-tee をビルド中..."
    cargo build --release --bin title-tee
  fi
  echo "  title-cli をビルド中..."
  cargo build --release --bin title-cli
  echo "  ビルド完了"
else
  echo "  SKIP: cargo が未インストール"
fi

# ---------------------------------------------------------------------------
# Step 3: Enclave イメージのビルド
# ---------------------------------------------------------------------------
echo ""
echo "[Step 3/10] Enclave イメージのビルド..."

EIF_PATH="$PROJECT_ROOT/title-tee.eif"
TEE_MEASUREMENTS="{}"

if command -v nitro-cli &>/dev/null; then
  docker build -t title-tee-enclave -f deploy/aws/docker/tee.Dockerfile .

  nitro-cli build-enclave \
    --docker-uri title-tee-enclave:latest \
    --output-file "$EIF_PATH" 2>&1 | tee /tmp/enclave-build.log

  echo "  EIF: $EIF_PATH"

  TEE_MEASUREMENTS=$(nitro-cli describe-eif --eif-path "$EIF_PATH" | \
    python3 -c "
import sys, json
m = json.load(sys.stdin).get('Measurements', {})
out = {k: m[k] for k in ['PCR0', 'PCR1', 'PCR2'] if k in m}
print(json.dumps(out))
" 2>/dev/null || echo "{}")
  echo "  測定値: $TEE_MEASUREMENTS"
else
  echo "  SKIP: nitro-cli 未インストール。MockRuntimeモード。"
fi

# ---------------------------------------------------------------------------
# Step 4: Enclave / TEE の起動
# ---------------------------------------------------------------------------
echo ""
echo "[Step 4/10] TEE の起動..."

ENCLAVE_CPU="${ENCLAVE_CPU_COUNT:-2}"
ENCLAVE_MEM="${ENCLAVE_MEMORY_MIB:-1024}"

if command -v nitro-cli &>/dev/null && [ -f "$EIF_PATH" ]; then
  # 既存Enclaveの停止
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

  ENCLAVE_OUTPUT=$(nitro-cli run-enclave \
    --eif-path "$EIF_PATH" \
    --cpu-count "$ENCLAVE_CPU" \
    --memory "$ENCLAVE_MEM")

  ENCLAVE_CID=$(echo "$ENCLAVE_OUTPUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['EnclaveCID'])")
  echo "  Enclave起動完了 (CID=$ENCLAVE_CID)"

  pkill -f "socat TCP-LISTEN:4000" 2>/dev/null || true
  sleep 1
  socat TCP-LISTEN:4000,fork,reuseaddr VSOCK-CONNECT:"$ENCLAVE_CID":4000 &
  echo "  インバウンドブリッジ起動 (TCP:4000 → vsock:$ENCLAVE_CID:4000)"
else
  echo "  MockRuntimeモード: TEEを直接起動"
  TEE_PID=$(pgrep -x title-tee 2>/dev/null || true)
  if [ -z "$TEE_PID" ]; then
    if [ -f "target/release/title-tee" ]; then
      TEE_RUNTIME=mock PROXY_ADDR=direct \
        SOLANA_RPC_URL="$SOLANA_RPC_URL" \
        CORE_COLLECTION_MINT="$CORE_COLLECTION_MINT" \
        EXT_COLLECTION_MINT="$EXT_COLLECTION_MINT" \
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
    echo "  TEE は既に稼働中 (PID=$TEE_PID)"
  fi
fi

# ---------------------------------------------------------------------------
# Step 5: Proxy の起動
# ---------------------------------------------------------------------------
echo ""
echo "[Step 5/10] Proxy の起動..."

if command -v nitro-cli &>/dev/null && [ -f "$EIF_PATH" ]; then
  if ! pgrep -f title-proxy &>/dev/null; then
    if [ -f "target/release/title-proxy" ]; then
      nohup ./target/release/title-proxy > ~/title-proxy.log 2>&1 &
      echo "  Proxy起動 (PID=$!)"
    else
      echo "  ERROR: Proxyバイナリが見つかりません"
    fi
  else
    echo "  Proxy は既に稼働中"
  fi
else
  echo "  SKIP: MockRuntimeモードではProxy不要"
fi

# ---------------------------------------------------------------------------
# Step 6: Docker Compose (Gateway)
# ---------------------------------------------------------------------------
echo ""
echo "[Step 6/10] Docker Compose (Gateway) 起動..."

# Auto-generated値を .env に書き出す（Docker コンテナが env_file 経由で読む）
# 冪等: 既に存在するキーは上書きしない
ensure_env() {
  local key="$1" value="$2"
  if ! grep -q "^${key}=" .env 2>/dev/null; then
    echo "${key}=${value}" >> .env
    echo "  .env に ${key} を追加"
  fi
}
ensure_env "GATEWAY_SIGNING_KEY" "$GATEWAY_SIGNING_KEY"
ensure_env "GLOBAL_CONFIG_PDA" "$GLOBAL_CONFIG_PDA"

docker compose -f deploy/aws/docker-compose.production.yml up -d --build
echo "  Docker Compose 起動完了"

echo "  サービスの起動を待機中..."
for i in $(seq 1 30); do
  if curl -sf -X POST -H "Content-Type: application/json" \
    -d '{"content_size":1,"content_type":"image/jpeg"}' \
    http://localhost:3000/upload-url > /dev/null 2>&1; then
    echo "  Gateway 応答確認"
    break
  fi
  sleep 2
done

# ---------------------------------------------------------------------------
# Step 7: S3バケットの確認
# ---------------------------------------------------------------------------
echo ""
echo "[Step 7/10] S3ストレージの確認..."

BUCKET_NAME="${S3_BUCKET:-title-uploads}"
if echo "$S3_ENDPOINT" | grep -q "s3.amazonaws.com\|s3\..*\.amazonaws\.com"; then
  echo "  S3バケット: $BUCKET_NAME"
  if aws s3 ls "s3://$BUCKET_NAME" > /dev/null 2>&1; then
    echo "  OK"
  else
    echo "  WARNING: S3バケットにアクセスできません"
  fi
else
  echo "  S3互換エンドポイント: $S3_ENDPOINT"
  docker compose exec -T minio sh -c '
    mc alias set local http://localhost:9000 '"$S3_ACCESS_KEY"' '"$S3_SECRET_KEY"' 2>/dev/null
    mc mb local/title-uploads --ignore-existing 2>/dev/null
  ' 2>/dev/null && echo "  OK" || echo "  WARNING: バケット作成失敗"
fi

# ---------------------------------------------------------------------------
# Step 8: TEEノード登録 (/register-node → DAO署名)
#
# データの流れ:
#   network.json → program_id, authority（ローカル設定、GlobalConfigからではない）
#   TEE/Gateway  → signing_pubkey, encryption_pubkey, measurements（自分自身の情報）
#   f(自分の情報) = 登録TX → GlobalConfigに追加を申請
#
# Authority keypair が存在する場合: 即署名+ブロードキャスト（devnet）
# 存在しない場合: DAOのガバナンスシステムに審査TXを送信（mainnet）
# ---------------------------------------------------------------------------
echo ""
echo "[Step 8/10] TEEノード登録..."

# EC2メタデータから公開IPを自動取得（IMDSv2）
if [ -z "${PUBLIC_ENDPOINT:-}" ]; then
  TOKEN=$(curl -s -X PUT "http://169.254.169.254/latest/api/token" \
    -H "X-aws-ec2-metadata-token-ttl-seconds: 21600" 2>/dev/null) || true
  if [ -n "$TOKEN" ]; then
    PUBLIC_IP=$(curl -s -H "X-aws-ec2-metadata-token: $TOKEN" \
      http://169.254.169.254/latest/meta-data/public-ipv4 2>/dev/null) || true
  fi
  if [ -n "${PUBLIC_IP:-}" ]; then
    PUBLIC_ENDPOINT="http://${PUBLIC_IP}:3000"
    echo "  公開IP自動取得: ${PUBLIC_IP}"
  else
    PUBLIC_ENDPOINT="http://localhost:3000"
    echo "  WARNING: 公開IP取得失敗、localhost使用"
  fi
fi

./target/release/title-cli register-node \
  --tee-url http://localhost:4000 \
  --gateway-endpoint "$PUBLIC_ENDPOINT" \
  ${TEE_MEASUREMENTS:+--measurements "$TEE_MEASUREMENTS"} \
  2>&1 || true

# ---------------------------------------------------------------------------
# Step 9: Merkle Tree 作成
# ---------------------------------------------------------------------------
echo ""
echo "[Step 9/10] Merkle Tree 作成..."

./target/release/title-cli create-tree \
  --tee-url http://localhost:4000 \
  --max-depth 14 \
  --max-buffer-size 64 \
  2>&1 || true

# ---------------------------------------------------------------------------
# Step 10: ヘルスチェック
# ---------------------------------------------------------------------------
echo ""
echo "[Step 10/10] ヘルスチェック..."

# Solana RPC
if curl -sf -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  "$SOLANA_RPC_URL" > /dev/null 2>&1; then
  echo "  OK  Solana RPC"
else
  echo "  NG  Solana RPC ($SOLANA_RPC_URL)"
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

echo ""
echo "=== ノード起動完了 ==="
echo ""
echo "Gateway: http://$(curl -sf http://169.254.169.254/latest/meta-data/public-ipv4 2>/dev/null || echo 'localhost'):3000"
echo ""
