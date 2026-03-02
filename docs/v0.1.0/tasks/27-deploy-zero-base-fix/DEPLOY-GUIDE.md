# Title Protocol Nitro TEE デプロイガイド

新規 AWS EC2 インスタンスに Title Protocol を構築し、Nitro TEE で動作確認するまでの手順書。

---

## 前提条件

ローカルマシンに以下がインストール済みであること。

| ツール | 用途 | インストール |
|--------|------|-------------|
| AWS CLI | AWS操作 | `brew install awscli` → `aws configure` |
| Terraform >= 1.5 | インフラ構築 | `brew install terraform` |
| Node.js >= 20 | 動作確認スクリプト | `brew install node` |
| Solana CLI | ウォレット操作 | [公式](https://docs.anza.xyz/cli/install) |

AWS アカウントに以下が必要。

- Nitro Enclave 対応リージョン（`ap-northeast-1` 推奨）
- EC2 キーペアが登録済み
- [Helius](https://helius.dev/) の Solana Devnet RPC API キー（無料枠あり）
- Solana Devnet ウォレットに **2 SOL 以上**

---

## 1. SSH キーペアの準備

既にキーペアがある場合はこのステップをスキップ。

```bash
mkdir -p deploy/aws/keys

aws ec2 create-key-pair \
  --key-name title-protocol-devnet \
  --query 'KeyMaterial' \
  --output text > deploy/aws/keys/title-protocol-devnet.pem

chmod 400 deploy/aws/keys/title-protocol-devnet.pem
```

---

## 2. インフラ構築（Terraform）

```bash
cd deploy/aws/terraform
terraform init
terraform apply
```

`yes` を入力すると、以下のリソースが作成される。

| リソース | 用途 |
|---------|------|
| EC2 (c5.xlarge) | TEE + Gateway + Indexer |
| S3 バケット | 暗号化ペイロード一時保管（1日で自動削除） |
| IAM ロール | EC2 → S3 アクセス |
| IAM ユーザー + アクセスキー | Gateway → S3 presigned URL 生成 |
| Security Group | SSH(22), Gateway(3000), Indexer(5000) のみ許可 |

完了すると出力値が表示される。

```
instance_public_ip    = "XX.XX.XX.XX"
s3_access_key_id      = "AKIA..."
s3_secret_access_key  = <sensitive>
s3_bucket_name        = "title-uploads-devnet"
ssh_command           = "ssh -i ../keys/title-protocol-devnet.pem ec2-user@XX.XX.XX.XX"
```

S3 シークレットキーは以下で表示できる。

```bash
terraform output -raw s3_secret_access_key
```

---

## 3. EC2 接続と初期セットアップ確認

SSH で接続する。

```bash
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@<PUBLIC_IP>
```

初回起動時に `user-data.sh` が自動実行され、Docker, Nitro CLI, Node.js, Rust, gcc, socat 等がインストールされる。完了を確認する。

```bash
tail -f /var/log/title-setup.log
# 「=== 初期セットアップ完了 ===」が表示されれば OK（約3分）
```

---

## 4. リポジトリのクローン

```bash
git clone https://github.com/yudai-mori-2004/title-protocol.git
cd title-protocol
```

---

## 5. .env の作成

```bash
cp .env.example .env
vim .env
```

以下の値を設定する。`<...>` の部分をすべて置き換えること。

```bash
# --- 共通 ---
SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=<Helius APIキー>

# --- Gateway ---
GATEWAY_SIGNING_KEY=<openssl rand -hex 32 の出力>
TEE_ENDPOINT=http://localhost:4000
S3_ENDPOINT=https://s3.ap-northeast-1.amazonaws.com
S3_PUBLIC_ENDPOINT=https://s3.ap-northeast-1.amazonaws.com
S3_ACCESS_KEY=<terraform output s3_access_key_id の値>
S3_SECRET_KEY=<terraform output -raw s3_secret_access_key の値>
S3_BUCKET=title-uploads-devnet

# --- TEE ---
TEE_RUNTIME=nitro
MOCK_MODE=false
PROXY_ADDR=vsock:8000
COLLECTION_MINT=
GATEWAY_PUBKEY=
TRUSTED_EXTENSIONS=phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1
WASM_DIR=/wasm-modules

# --- DB ---
DB_USER=title
DB_PASSWORD=<openssl rand -base64 24 の出力>

# --- Indexer ---
DATABASE_URL=postgres://title:<上のDB_PASSWORD>@localhost:5432/title_indexer
DAS_ENDPOINTS=https://devnet.helius-rpc.com/?api-key=<Helius APIキー>

# --- Enclave ---
ENCLAVE_CPU_COUNT=2
ENCLAVE_MEMORY_MIB=1024
```

---

## 6. デプロイ実行

```bash
./deploy/aws/setup-ec2.sh
```

このスクリプトが自動で以下を実行する。

1. .env の読み込みと必須変数チェック
2. Solana ウォレットの自動作成（未作成の場合）
3. WASM モジュールのビルド（4モジュール）
4. ホスト側バイナリのビルド（title-proxy）
5. Enclave Docker イメージのビルド + EIF 変換
6. Enclave の起動 + vsock ブリッジ
7. title-proxy の起動
8. Docker Compose（Gateway + PostgreSQL + Indexer）の起動
9. Global Config 初期化 + Merkle Tree 作成
10. ヘルスチェック

初回ビルドは約 **20〜25分** かかる。

ヘルスチェックで Gateway / TEE / Solana RPC が OK になれば成功。

---

## 7. SOL の送金

setup-ec2.sh の最後に TEE ウォレットアドレスが表示される。
このウォレットに Devnet SOL を送金する。

```bash
# EC2 上で確認
solana-keygen pubkey   # EC2ペイヤーウォレット

# ローカルマシンから送金
solana transfer <EC2ペイヤーアドレス> 1 --allow-unfunded-recipient --url devnet
solana transfer <TEEウォレットアドレス> 1 --allow-unfunded-recipient --url devnet
```

SOL が不足すると Merkle Tree の作成が失敗する。
その場合は送金後に init-config を再実行する（下記「Merkle Tree の再作成」参照）。

---

## 8. 動作確認

**ローカルマシンから**実行する。

```bash
cd experiments
npm install   # 初回のみ

npx tsx register-photo.ts <PUBLIC_IP> <画像パス> \
  --wallet ~/.config/solana/id.json \
  --rpc "https://devnet.helius-rpc.com/?api-key=<Helius APIキー>" \
  --encryption-pubkey "<tee-info.json の encryption_pubkey>"
```

`encryption_pubkey` は EC2 上の `tests/e2e/fixtures/tee-info.json` に記載されている。

```bash
# EC2 上で確認
cat ~/title-protocol/tests/e2e/fixtures/tee-info.json
```

成功すると以下のように表示される。

```
STEP 4 /verify 完了 (302ms)
  tee_type:       aws_nitro
STEP 5 Arweaveにアップロード → https://gateway.irys.xyz/...
STEP 6 /sign 完了 (1351ms)
DONE 全結果を保存: output-register.json
```

`tee_type: aws_nitro` が表示されれば Nitro TEE が正常動作している。

---

## トラブルシューティング

### setup-ec2.sh で「必須環境変数が未設定」

.env に空の値がある。特に `S3_ACCESS_KEY`, `S3_SECRET_KEY`, `GATEWAY_SIGNING_KEY`, `DB_PASSWORD` を確認する。

### Merkle Tree の再作成

SOL 不足で Merkle Tree 作成が失敗した場合、TEE が「active」状態のまま固定される。
Enclave を再起動してから init-config を再実行する。

```bash
# Enclave の再起動
ENCLAVE_ID=$(nitro-cli describe-enclaves | python3 -c "import sys,json; [print(e['EnclaveID']) for e in json.load(sys.stdin) if e.get('State')=='RUNNING']")
nitro-cli terminate-enclave --enclave-id $ENCLAVE_ID

pkill -f "socat TCP-LISTEN:4000"
pkill -f title-proxy
sleep 2

# 再起動
ENCLAVE_OUTPUT=$(nitro-cli run-enclave --eif-path ~/title-protocol/title-tee.eif --cpu-count 2 --memory 1024 --debug-mode)
ENCLAVE_CID=$(echo "$ENCLAVE_OUTPUT" | python3 -c "import sys,json; print(json.load(sys.stdin)['EnclaveCID'])")
nohup socat TCP-LISTEN:4000,fork,reuseaddr VSOCK-CONNECT:$ENCLAVE_CID:4000 > /tmp/socat.log 2>&1 &
cd ~/title-protocol && nohup ./target/release/title-proxy > /tmp/title-proxy.log 2>&1 &

# TEEウォレットにSOLを送金（アドレスが再生成されるため新しいアドレスを確認）
sleep 10
curl -s http://localhost:4000/health  # "ok" を確認

# init-config 再実行
set -a && source .env && set +a
node scripts/init-config.mjs --rpc "$SOLANA_RPC_URL" --gateway http://localhost:3000 --tee http://localhost:4000
```

### /verify で「early eof」

TEE が S3 からファイルをダウンロードできない。title-proxy が起動しているか確認する。

```bash
pgrep -f title-proxy || (cd ~/title-protocol && nohup ./target/release/title-proxy > /tmp/title-proxy.log 2>&1 &)
```

### curl localhost:4000/health が応答しない

1. Enclave が動作しているか: `nitro-cli describe-enclaves`
2. socat ブリッジが動いているか: `pgrep -f "socat TCP-LISTEN:4000"`
3. コンソールログを確認:
   ```bash
   nitro-cli console --enclave-id $(nitro-cli describe-enclaves | python3 -c "import sys,json; [print(e['EnclaveID']) for e in json.load(sys.stdin) if e.get('State')=='RUNNING']")
   ```

### Docker Compose でポート衝突

```bash
docker ps -a                    # 古いコンテナを確認
docker compose -f deploy/aws/docker-compose.production.yml down
docker compose -f deploy/aws/docker-compose.production.yml up -d
```

---

## インスタンスの削除

使い終わったら全リソースを削除する。

```bash
# 1. S3 バケットを空にする（中身があると削除できない）
aws s3 rm s3://title-uploads-devnet --recursive

# 2. 全リソースを削除
cd deploy/aws/terraform
terraform destroy
```

`yes` を入力すると EC2, S3, IAM ユーザー, Security Group がすべて削除される。

---

## アーキテクチャ

```
Client → Gateway(:3000)
              ↓ HTTP
         socat (ホスト TCP:4000 → vsock:CID:4000)
              ↓ vsock
         ┌──── Nitro Enclave ────┐
         │ socat → title-tee     │
         │          ↓            │
         │ socat → vsock:3:8000  │
         └───────────────────────┘
              ↓ vsock
         title-proxy (ホスト vsock:8000)
              ↓ HTTPS
         Solana RPC / S3 / Arweave
```

- **Gateway**: HTTP API。クライアントからのリクエストを受けて TEE に中継する。
- **TEE**: Nitro Enclave 内で動作。コンテンツの検証・署名を行う。
- **title-proxy**: TEE からの外部通信を vsock 経由で中継する。
- **socat**: TCP と vsock を相互変換するブリッジ。ホスト側と Enclave 内の2箇所で動作する。
