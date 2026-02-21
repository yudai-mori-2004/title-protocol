# Title Protocol ノード構築 → コンテンツ登録 完全手順書

EC2インスタンス上に Title Protocol ノードを新規構築し、
ローカルマシンからC2PAコンテンツをDevnetに登録するまでの全手順。

2026-02-22 時点の実績に基づく。

---

## 前提条件

### EC2 インスタンス

| 項目 | 値 |
|------|-----|
| インスタンスタイプ | `c5.xlarge` 以上 |
| AMI | Amazon Linux 2023 |
| リージョン | ap-northeast-1 |
| Security Group | Inbound: 22 (SSH), 3000 (Gateway), 4000 (TEE) |
| S3バケット | `title-uploads-devnet`（同リージョン、パブリック読み取り不要） |
| IAMロール | S3バケットへの読み書き権限、または IAM ユーザーのアクセスキー |

### ローカルマシン

| 項目 | 値 |
|------|-----|
| Node.js | v20 以上（`crypto.subtle` が必要。v18は非対応） |
| Solana CLI | インストール済み |
| SSH鍵 | EC2に接続できるpemファイル |

### 外部サービス

| 項目 | 値 |
|------|-----|
| Solana RPC | Helius devnet 推奨（レート制限が緩い） |
| Anchor Program | `C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN` (デプロイ済み) |

---

## 全体フロー

```
[EC2セットアップ]
  1. リポジトリクローン + .env 設定
  2. Rustバイナリビルド（title-tee, title-gateway）
  3. WASMモジュールビルド
  4. TEE起動（MockRuntime）
  5. Gateway起動
  6. Global Config初期化 + Merkle Tree作成

[ローカルからコンテンツ登録]
  7. tee-info.json をEC2から取得
  8. SDK ビルド
  9. register-content.mjs でコンテンツ登録
```

---

## Part 1: EC2 ノードセットアップ

### Step 1: リポジトリクローン + 依存インストール

```bash
ssh -i <your-key.pem> ec2-user@<EC2_IP>

# Rust インストール
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup target add wasm32-unknown-unknown

# Node.js インストール（nvm経由）
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
source ~/.nvm/nvm.sh
nvm install 20

# Docker インストール（Amazon Linux 2023）
sudo dnf install -y docker
sudo systemctl enable --now docker
sudo usermod -aG docker ec2-user
# ※ ここで再ログインが必要

# リポジトリクローン
git clone https://github.com/<org>/title-protocol.git ~/title-protocol
cd ~/title-protocol
```

### Step 2: .env 設定

```bash
cp .env.example .env
vim .env
```

**必須設定項目:**

```bash
# Solana RPC（Helius devnet推奨）
SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=<YOUR_HELIUS_API_KEY>

# Gateway認証鍵（ランダム生成）
GATEWAY_SIGNING_KEY=$(openssl rand -hex 32)

# S3互換ストレージ（AWS S3 の場合）
S3_ENDPOINT=https://s3.ap-northeast-1.amazonaws.com
S3_PUBLIC_ENDPOINT=https://s3.ap-northeast-1.amazonaws.com
S3_ACCESS_KEY=<AWS_ACCESS_KEY_ID>
S3_SECRET_KEY=<AWS_SECRET_ACCESS_KEY>
S3_BUCKET=title-uploads-devnet

# TEE設定（MockRuntime）
TEE_RUNTIME=mock
MOCK_MODE=true
PROXY_ADDR=direct

# WASM拡張モジュール
TRUSTED_EXTENSIONS=phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1

# DB（Indexer用、省略可）
DB_PASSWORD=$(openssl rand -base64 24)
DATABASE_URL=postgres://title:${DB_PASSWORD}@localhost:5432/title_indexer
```

> **注意**: `COLLECTION_MINT` と `GATEWAY_PUBKEY` は Step 6 の init-config.mjs 実行後に自動設定されるため、初回は空でよい。

### Step 3: Rust バイナリビルド

```bash
cd ~/title-protocol

# title-tee と title-gateway をリリースビルド
OPENSSL_NO_VENDOR=1 cargo build --release --bin title-tee --bin title-gateway

# バイナリを /usr/local/bin にコピー（任意）
sudo cp target/release/title-tee target/release/title-gateway /usr/local/bin/
```

> ビルド時間: c5.xlarge で約9分。

### Step 4: WASM モジュールビルド

```bash
WASM_OUTPUT=~/title-protocol/wasm-modules
mkdir -p "$WASM_OUTPUT"

export OPENSSL_NO_VENDOR=1
for module in phash-v1 hardware-google c2pa-training-v1 c2pa-license-v1; do
  echo "Building: $module"
  (cd "wasm/$module" && cargo build --target wasm32-unknown-unknown --release)
  cp "wasm/$module/target/wasm32-unknown-unknown/release/${module//-/_}.wasm" \
     "$WASM_OUTPUT/$module.wasm"
done

ls -la "$WASM_OUTPUT/"
# 4つの .wasm ファイルが存在すること
```

### Step 5: TEE + Gateway 起動

```bash
cd ~/title-protocol
set -a && source .env && set +a

# --- TEE 起動 ---
MOCK_MODE=true TEE_RUNTIME=mock PROXY_ADDR=direct \
  SOLANA_RPC_URL="$SOLANA_RPC_URL" \
  COLLECTION_MINT="${COLLECTION_MINT:-}" \
  GATEWAY_PUBKEY="${GATEWAY_PUBKEY:-}" \
  TRUSTED_EXTENSIONS="${TRUSTED_EXTENSIONS:-phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1}" \
  WASM_DIR="$HOME/title-protocol/wasm-modules" \
  nohup /usr/local/bin/title-tee > /tmp/title-tee.log 2>&1 &
echo "TEE PID=$!"

sleep 3

# TEE ヘルスチェック
curl -sf http://localhost:4000/health && echo " OK" || echo " FAIL"

# --- Gateway 起動 ---
S3_ENDPOINT="$S3_ENDPOINT" \
  S3_PUBLIC_ENDPOINT="$S3_PUBLIC_ENDPOINT" \
  S3_ACCESS_KEY="$S3_ACCESS_KEY" \
  S3_SECRET_KEY="$S3_SECRET_KEY" \
  S3_BUCKET="$S3_BUCKET" \
  TEE_ENDPOINT="http://localhost:4000" \
  GATEWAY_SIGNING_KEY="$GATEWAY_SIGNING_KEY" \
  SOLANA_RPC_URL="$SOLANA_RPC_URL" \
  nohup /usr/local/bin/title-gateway > /tmp/title-gateway.log 2>&1 &
echo "Gateway PID=$!"

sleep 3

# Gateway ヘルスチェック
curl -sf http://localhost:3000/.well-known/title-node-info | python3 -m json.tool
```

**期待される出力:**

```json
{
    "signing_pubkey": "<Base58の公開鍵>",
    "supported_extensions": [
        "core-c2pa", "phash-v1", "hardware-google",
        "c2pa-training-v1", "c2pa-license-v1"
    ],
    "limits": {
        "max_single_content_bytes": 104857600,
        "max_concurrent_bytes": 536870912
    }
}
```

### Step 6: Global Config 初期化 + Merkle Tree 作成

```bash
cd ~/title-protocol

# scripts/ の依存インストール
(cd scripts && npm install)

# Solana キーペア準備（なければ生成 + airdrop）
solana config set --url "$SOLANA_RPC_URL"
if [ ! -f ~/.config/solana/id.json ]; then
  solana-keygen new --no-bip39-passphrase --silent
  solana airdrop 5
fi

# SOL残高確認（Global Config初期化 + Tree作成に ~0.5 SOL 必要）
solana balance

# TEE wallet にも SOL を送金（/create-tree のpayer）
TEE_WALLET=$(curl -sf http://localhost:4000/health -o /dev/null && \
  curl -sf http://localhost:3000/.well-known/title-node-info | python3 -c "
import sys,json; print(json.load(sys.stdin)['signing_pubkey'])" 2>/dev/null)
echo "TEE wallet: $TEE_WALLET"
# TEE walletのアドレスは init-config.mjs 実行時にログに表示される

# Global Config + Merkle Tree 初期化
node scripts/init-config.mjs \
  --rpc "$SOLANA_RPC_URL" \
  --gateway "http://localhost:3000" \
  --tee "http://localhost:4000"
```

**期待される出力:**

```
  Authority: <ウォレットアドレス>
  Global Config PDA: <PDAアドレス>
  Global Config は既に存在します (または「作成完了」)
  Gateway ノード情報を取得中...
  TEE signing_pubkey: <Base58鍵>
  TEEノード情報を登録中...
  TEEノード登録完了
  Merkle Tree 作成中...
  POST http://localhost:4000/create-tree → 200
  Tree Address: <Merkle Treeアドレス>
  tee-info.json 保存完了
```

> **重要**: `init-config.mjs` は `tests/e2e/fixtures/tee-info.json` を生成する。
> このファイルにはTEEの暗号化公開鍵・署名公開鍵・Treeアドレスが含まれ、
> クライアント（ローカルマシン）での登録に必要。

---

## Part 2: ローカルからコンテンツ登録

### Step 7: tee-info.json をEC2から取得

```bash
# ローカルマシンで実行
scp -i <your-key.pem> \
  ec2-user@<EC2_IP>:~/title-protocol/tests/e2e/fixtures/tee-info.json \
  tests/e2e/fixtures/tee-info.json
```

`tee-info.json` の中身を確認:

```bash
cat tests/e2e/fixtures/tee-info.json
```

```json
{
  "signing_pubkey": "6MBPB4dTVfARrLpetHjG2RvUaKDdvXGUQEig7LsRnE8R",
  "encryption_pubkey": "1/BY7hZZx9PZqxV/tUdTjOcLjO1qETZQjle1hAbz3Co=",
  "tree_address": "edqopbyxr4p9YN42iazuD4MQ3E2uKwXHCowfUeVcwyF"
}
```

### Step 8: SDK ビルド

```bash
cd sdk/ts && npm install && npm run build && cd ../..
```

### Step 9: コンテンツ登録

```bash
# Node.js v20以上を使用（v18は crypto.subtle 非対応）
# nvm use 20 (or nvm use 24)

GATEWAY_URL=http://<EC2_IP>:3000 \
SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=<YOUR_KEY> \
  node scripts/register-content.mjs <image.jpg> --processor core-c2pa,phash-v1
```

**フルフロー（内部で実行される11ステップ）:**

```
Step 0: Gateway /.well-known/title-node-info 取得
Step 1: ウォレット準備 (~/.config/solana/id.json から読み込み)
Step 2: ClientPayload構築 + E2EE暗号化 (ECDH + HKDF + AES-256-GCM)
Step 3: 暗号化ペイロード → S3 presigned URL でアップロード
Step 4: POST /verify (core-c2pa, phash-v1) → TEEがC2PA検証 + WASM実行
Step 5: レスポンス復号 (AES-256-GCM) → signed_json 取得
Step 6: signed_json → S3 に保存（本番ではArweave等を使用）
Step 7: POST /sign → TEEが signed_json_uri を検証し、Bubblegum TX に部分署名
Step 8: ウォレット署名 + Solana devnet にブロードキャスト
```

**期待される出力:**

```
=== Title Protocol コンテンツ登録 (Client E2E) ===
  Image: /path/to/image.jpg
  Processors: core-c2pa, phash-v1
  Gateway: http://<EC2_IP>:3000
  Solana RPC: https://devnet.helius-rpc.com/?api-key=xxx

--- Step 0: ノード情報取得 ---
  Gateway signing_pubkey: <Base58>
  Supported extensions: core-c2pa, phash-v1, ...
  TEE signing_pubkey: <Base58>
  TEE encryption_pubkey: <Base64>
  Tree address: <Base58>
  Image size: X.XX MB

--- Step 1: ウォレット準備 ---
  Wallet (from id.json): <ウォレットアドレス>
  Balance: XX.XX SOL

--- Step 2: ペイロード暗号化 ---
  ClientPayload size: X.XX MB
  Encrypted size: X.XX MB

--- Step 3: Temporary Storage アップロード ---
  S3 presigned URL 取得: OK
  S3 upload: OK

--- Step 4: /verify (core-c2pa, phash-v1) ---
  Processing... (C2PA検証 + WASM実行、数十秒かかります)
  /verify: OK

--- Step 5: レスポンス復号 ---
  Results: 2 processor(s)
    - core-c2pa
      content_hash: 0x...
      content_type: image/jpeg
      creator_wallet: <ウォレット>
      provenance_nodes: 1
    - phash-v1
      extension_id: phash-v1
      wasm_hash: 0x...

--- Step 6: signed_json → S3 保存 ---
  core-c2pa: stored (XXXX bytes)
  phash-v1: stored (XXXX bytes)

--- Step 7: /sign ---
  Partial TXs: 2

--- Step 8: ウォレット署名 + Solanaブロードキャスト ---
  TX 1: TEE署名確認OK
  TX 1: <signature>
    → https://explorer.solana.com/tx/<sig>?cluster=devnet
    ✓ Confirmed
  TX 2: TEE署名確認OK
  TX 2: <signature>
    → https://explorer.solana.com/tx/<sig>?cluster=devnet
    ✓ Confirmed

========================================
  登録完了
========================================
```

---

## トラブルシューティング

### 1. `crypto is not defined` (Node.js v18)

**症状**: `ReferenceError: crypto is not defined`

**原因**: Node.js v18 では `crypto.subtle` がグローバルに存在しない。

**対策**: Node.js v20 以上を使用する。

```bash
nvm use 20  # または nvm use 24
```

### 2. TEE再起動後に `/create-tree` が 409 を返す

**症状**: `HTTP 409 - TEEは既にactive状態です`

**原因**: `/create-tree` は TEE のライフサイクル中に1回だけ呼び出し可能。
TEEを再起動しても Global Config 上の Tree は残存しているため、
`init-config.mjs` は `/create-tree` の409を無視して既存のTree情報を使う。

**対策**: 409 は正常動作。`tee-info.json` の `tree_address` が正しいことを確認すればよい。

### 3. Gateway が S3 presigned URL 生成に失敗

**症状**: `/upload-url` が HTTP 500 を返す。

**確認**:

```bash
# EC2上で Gateway ログ確認
tail -50 /tmp/title-gateway.log
```

**よくある原因**:
- `.env` の `S3_ENDPOINT` / `S3_ACCESS_KEY` / `S3_SECRET_KEY` が古い `MINIO_*` のまま
- S3バケットが存在しない、またはリージョンが異なる
- IAMロールの権限不足

### 4. `/verify` が HTTP 502 を返す

**症状**: Gateway → TEE の中継で 502。

**確認**:

```bash
# TEE ヘルスチェック
curl http://<EC2_IP>:4000/health

# TEE ログ確認
tail -100 /tmp/title-tee.log
```

**よくある原因**:
- TEEプロセスが落ちている（メモリ不足等）
- WASMモジュールが `WASM_DIR` に存在しない
- 画像が C2PA Active Manifest を含まない（TEEはC2PA検証を行うため）

### 5. Solana TX が失敗する

**症状**: `Transaction simulation failed` または `blockhash not found`

**対策**:
- SOL 残高を確認（`solana balance`）。各TXに ~0.01 SOL 必要。
- `blockhash not found` は RPC のレイテンシが原因。再実行で解決することが多い。
- Helius 等の専用 RPC を使用する（公式 devnet RPC はレート制限が厳しい）。

### 6. EC2で `.env` の環境変数が古い

**症状**: Gateway起動後もS3アクセスが `MINIO_*` を参照。

**確認**:

```bash
# 環境変数を確認（起動中のプロセスの環境を見る）
cat /proc/$(pgrep -x title-gateway)/environ | tr '\0' '\n' | grep S3_
```

**対策**: `.env` を更新後、プロセスを再起動する。`source .env` だけではnohupプロセスに反映されない。

---

## 環境変数一覧（ノード運用に必要な全変数）

### Gateway

| 変数 | 説明 | 例 |
|------|------|-----|
| `S3_ENDPOINT` | S3互換ストレージのエンドポイント | `https://s3.ap-northeast-1.amazonaws.com` |
| `S3_PUBLIC_ENDPOINT` | クライアント向けエンドポイント（省略時はS3_ENDPOINTと同じ） | 同上 |
| `S3_ACCESS_KEY` | アクセスキー | `AKIA...` |
| `S3_SECRET_KEY` | シークレットキー | `Eg3c...` |
| `S3_BUCKET` | バケット名 | `title-uploads-devnet` |
| `TEE_ENDPOINT` | TEEの内部URL | `http://localhost:4000` |
| `GATEWAY_SIGNING_KEY` | Ed25519秘密鍵（64文字hex） | `openssl rand -hex 32` |
| `SOLANA_RPC_URL` | Solana RPC（sign-and-mint用、省略可） | `https://devnet.helius-rpc.com/...` |

### TEE

| 変数 | 説明 | 例 |
|------|------|-----|
| `TEE_RUNTIME` | ランタイム種別 | `mock` or `nitro` |
| `MOCK_MODE` | MockRuntime使用フラグ | `true` |
| `PROXY_ADDR` | プロキシアドレス | `direct` or `vsock:8000` |
| `SOLANA_RPC_URL` | Solana RPC | `https://devnet.helius-rpc.com/...` |
| `COLLECTION_MINT` | コレクションMintアドレス | init-config.mjs が設定 |
| `GATEWAY_PUBKEY` | Gateway公開鍵（省略可） | init-config.mjs が設定 |
| `TRUSTED_EXTENSIONS` | 許可するWASM拡張（CSV） | `phash-v1,hardware-google,...` |
| `WASM_DIR` | WASMモジュールのディレクトリ | `~/title-protocol/wasm-modules` |

---

## ノード再起動手順

TEEはステートレス設計のため、再起動すると鍵ペアが再生成される。

```bash
ssh -i <key.pem> ec2-user@<EC2_IP>

cd ~/title-protocol
set -a && source .env && set +a

# 1. 既存プロセス停止
pkill -x title-tee 2>/dev/null || true
pkill -x title-gateway 2>/dev/null || true
sleep 2

# 2. TEE 再起動（Step 5 と同じコマンド）
MOCK_MODE=true TEE_RUNTIME=mock PROXY_ADDR=direct \
  SOLANA_RPC_URL="$SOLANA_RPC_URL" \
  COLLECTION_MINT="${COLLECTION_MINT:-}" \
  GATEWAY_PUBKEY="${GATEWAY_PUBKEY:-}" \
  TRUSTED_EXTENSIONS="${TRUSTED_EXTENSIONS:-phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1}" \
  WASM_DIR="$HOME/title-protocol/wasm-modules" \
  nohup /usr/local/bin/title-tee > /tmp/title-tee.log 2>&1 &
sleep 3

# 3. Gateway 再起動
S3_ENDPOINT="$S3_ENDPOINT" \
  S3_PUBLIC_ENDPOINT="$S3_PUBLIC_ENDPOINT" \
  S3_ACCESS_KEY="$S3_ACCESS_KEY" \
  S3_SECRET_KEY="$S3_SECRET_KEY" \
  S3_BUCKET="$S3_BUCKET" \
  TEE_ENDPOINT="http://localhost:4000" \
  GATEWAY_SIGNING_KEY="$GATEWAY_SIGNING_KEY" \
  SOLANA_RPC_URL="$SOLANA_RPC_URL" \
  nohup /usr/local/bin/title-gateway > /tmp/title-gateway.log 2>&1 &
sleep 3

# 4. Global Config 再登録（鍵が変わるため必須）
node scripts/init-config.mjs \
  --rpc "$SOLANA_RPC_URL" \
  --gateway "http://localhost:3000" \
  --tee "http://localhost:4000"

# 5. ヘルスチェック
curl -sf http://localhost:4000/health && echo " TEE OK"
curl -sf http://localhost:3000/.well-known/title-node-info | python3 -m json.tool
```

再起動後、ローカルの `tee-info.json` を再取得すること（Step 7）。
