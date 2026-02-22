# AWS ノードセットアップ手順

EC2インスタンス上に Title Protocol ノード（Mock TEE）を構築し、
外部からコンテンツ登録可能な状態にするまでの手順。

2026-02-22 時点。`deploy/aws/` へのベンダー分離後の構成に基づく。

---

## 前提条件

### EC2 インスタンス

| 項目 | 値 |
|------|-----|
| インスタンスタイプ | `c5.xlarge` 以上（ビルドに4vCPU推奨） |
| AMI | Amazon Linux 2023 |
| Security Group | Inbound: 22 (SSH), 3000 (Gateway), 4000 (TEE Mock) |
| S3バケット | 同リージョンに作成済み（例: `title-uploads-devnet`） |
| IAMロール | S3バケットへの `s3:PutObject` / `s3:GetObject` 権限 |

> **Security Group 注意**: port 4000 (TEE直接アクセス) は本番では不要。
> Mock TEE の実験時のみ開放する。Nitro Enclave 運用時は Gateway (3000) のみ。

### 外部サービス

| 項目 | 値 |
|------|-----|
| Solana RPC | Helius devnet 推奨（公式RPCはレート制限が厳しい） |
| Anchor Program | `C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN` (デプロイ済み) |

---

## Step 1: 依存インストール

```bash
ssh -i <your-key.pem> ec2-user@<EC2_IP>

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup target add wasm32-unknown-unknown

# Node.js (nvm 経由、v20以上必須)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
source ~/.nvm/nvm.sh
nvm install 20

# リポジトリクローン
git clone https://github.com/<org>/title-protocol.git ~/title-protocol
cd ~/title-protocol
```

## Step 2: 環境変数設定

```bash
cp .env.example .env
vim .env
```

**最小限の設定（Mock TEE + AWS S3）:**

```bash
# --- 共通 ---
SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=<YOUR_KEY>

# --- Gateway ---
GATEWAY_SIGNING_KEY=$(openssl rand -hex 32)
TEE_ENDPOINT=http://localhost:4000
S3_ENDPOINT=https://s3.<REGION>.amazonaws.com
S3_PUBLIC_ENDPOINT=https://s3.<REGION>.amazonaws.com
S3_ACCESS_KEY=<AWS_ACCESS_KEY_ID>
S3_SECRET_KEY=<AWS_SECRET_ACCESS_KEY>
S3_BUCKET=title-uploads-devnet

# --- TEE (Mock) ---
TEE_RUNTIME=mock
MOCK_MODE=true
PROXY_ADDR=direct
TRUSTED_EXTENSIONS=phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1
WASM_DIR=$HOME/title-protocol/wasm-modules
```

> `COLLECTION_MINT` と `GATEWAY_PUBKEY` は Step 6 で自動設定されるため空でよい。

## Step 3: Rust バイナリビルド

```bash
cd ~/title-protocol

# Gateway と TEE のリリースビルド
OPENSSL_NO_VENDOR=1 cargo build --release --bin title-tee --bin title-gateway

# PATH に配置（任意）
sudo cp target/release/title-tee target/release/title-gateway /usr/local/bin/
```

> ビルド時間: c5.xlarge で約9分。

## Step 4: WASM モジュールビルド

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

## Step 5: TEE + Gateway 起動

```bash
cd ~/title-protocol
set -a && source .env && set +a

# --- TEE 起動 ---
MOCK_MODE=true TEE_RUNTIME=mock PROXY_ADDR=direct \
  SOLANA_RPC_URL="$SOLANA_RPC_URL" \
  COLLECTION_MINT="${COLLECTION_MINT:-}" \
  GATEWAY_PUBKEY="${GATEWAY_PUBKEY:-}" \
  TRUSTED_EXTENSIONS="${TRUSTED_EXTENSIONS}" \
  WASM_DIR="$HOME/title-protocol/wasm-modules" \
  nohup /usr/local/bin/title-tee > /tmp/title-tee.log 2>&1 &
echo "TEE PID=$!"
sleep 3

# ヘルスチェック
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

# ヘルスチェック
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

## Step 6: Global Config 初期化 + Merkle Tree 作成

```bash
cd ~/title-protocol

# scripts/ の依存インストール
(cd scripts && npm install)

# Solana キーペア準備
solana config set --url "$SOLANA_RPC_URL"
if [ ! -f ~/.config/solana/id.json ]; then
  solana-keygen new --no-bip39-passphrase --silent
  solana airdrop 5
fi

# SOL残高確認（Global Config + Tree作成に ~0.5 SOL 必要）
solana balance

# Global Config + Merkle Tree 初期化
node scripts/init-config.mjs \
  --rpc "$SOLANA_RPC_URL" \
  --gateway "http://localhost:3000" \
  --tee "http://localhost:4000"
```

**期待される出力:**

```
  Global Config PDA: <PDAアドレス>
  TEE signing_pubkey: <Base58>
  Merkle Tree 作成中...
  Tree Address: <Base58>
  tee-info.json 保存完了
```

## Step 7: 外部からのアクセス確認

ローカルマシンから疎通確認:

```bash
# Gateway
curl -sf http://<EC2_IP>:3000/.well-known/title-node-info | python3 -m json.tool

# TEE (Mock直接アクセス — 実験時のみ)
curl -sf http://<EC2_IP>:4000/health
```

両方応答すればノードセットアップ完了。
`experiments/register-photo.ts` でコンテンツ登録が可能な状態。

---

## ノード再起動手順

TEEはステートレス設計のため、再起動すると鍵ペアが再生成される。

```bash
cd ~/title-protocol
set -a && source .env && set +a

# 1. 停止
pkill -x title-tee 2>/dev/null || true
pkill -x title-gateway 2>/dev/null || true
sleep 2

# 2. TEE 再起動
MOCK_MODE=true TEE_RUNTIME=mock PROXY_ADDR=direct \
  SOLANA_RPC_URL="$SOLANA_RPC_URL" \
  COLLECTION_MINT="${COLLECTION_MINT:-}" \
  GATEWAY_PUBKEY="${GATEWAY_PUBKEY:-}" \
  TRUSTED_EXTENSIONS="${TRUSTED_EXTENSIONS}" \
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

---

## トラブルシューティング

### Gateway が S3 presigned URL 生成に失敗 (500)

```bash
tail -50 /tmp/title-gateway.log
```

- `.env` の `S3_ENDPOINT` / `S3_ACCESS_KEY` / `S3_SECRET_KEY` を確認
- S3バケットの存在とリージョン一致を確認
- IAMロールの権限不足（`s3:PutObject`, `s3:GetObject`）

### /verify が 502 を返す

```bash
curl http://localhost:4000/health
tail -100 /tmp/title-tee.log
```

- TEEプロセスが落ちていないか
- WASMモジュールが `WASM_DIR` に存在するか（4ファイル）
- 画像にC2PA Active Manifestが含まれているか

### TEE再起動後に /create-tree が 409

正常動作。`/create-tree` はTEEライフサイクル中に1回のみ。
`init-config.mjs` は409を無視して既存Tree情報を使用する。

### Solana TX 失敗 (blockhash not found)

- RPC のレイテンシが原因。再実行で解決することが多い
- Helius 等の専用RPC推奨（公式devnet RPCはレート制限が厳しい）
- SOL残高を確認: `solana balance`

---

## 環境変数一覧

### Gateway

| 変数 | 説明 | 例 |
|------|------|-----|
| `S3_ENDPOINT` | S3互換エンドポイント | `https://s3.ap-northeast-1.amazonaws.com` |
| `S3_PUBLIC_ENDPOINT` | クライアント向け（省略時=S3_ENDPOINT） | 同上 |
| `S3_ACCESS_KEY` | AWSアクセスキー | `AKIA...` |
| `S3_SECRET_KEY` | AWSシークレットキー | `Eg3c...` |
| `S3_BUCKET` | バケット名 | `title-uploads-devnet` |
| `TEE_ENDPOINT` | TEE内部URL | `http://localhost:4000` |
| `GATEWAY_SIGNING_KEY` | Ed25519秘密鍵 (64文字hex) | `openssl rand -hex 32` |
| `SOLANA_RPC_URL` | Solana RPC (sign-and-mint用) | `https://devnet.helius-rpc.com/...` |

### TEE

| 変数 | 説明 | 例 |
|------|------|-----|
| `TEE_RUNTIME` | ランタイム種別 | `mock` / `nitro` |
| `MOCK_MODE` | MockRuntime有効化 | `true` |
| `PROXY_ADDR` | プロキシ | `direct` / `vsock:8000` |
| `SOLANA_RPC_URL` | Solana RPC | `https://devnet.helius-rpc.com/...` |
| `COLLECTION_MINT` | コレクションMint | init-config.mjs が設定 |
| `GATEWAY_PUBKEY` | Gateway公開鍵 (Base58) | init-config.mjs が設定 |
| `TRUSTED_EXTENSIONS` | 許可WASM拡張 (CSV) | `phash-v1,hardware-google,...` |
| `WASM_DIR` | WASMモジュールディレクトリ | `~/title-protocol/wasm-modules` |
