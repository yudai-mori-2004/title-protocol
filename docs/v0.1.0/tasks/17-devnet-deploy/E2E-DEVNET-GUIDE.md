# Devnet E2Eテスト実行ガイド

EC2上でTitle ProtocolのE2Eテスト（SDK経由のC2PAコンテンツ登録フロー）を実行する手順。
2026-02-21にdevnetで全9テスト通過を確認済み。

## 前提条件

- EC2インスタンス (c5.xlarge, Amazon Linux 2023)
- Docker / Docker Compose インストール済み
- Rust toolchain インストール済み
- Node.js 20+ インストール済み
- Solana CLI 設定済み (devnet)
- `.env` に以下が設定済み:
  - `SOLANA_RPC_URL` (Helius devnet推奨)
  - `MINIO_ENDPOINT`, `MINIO_PUBLIC_ENDPOINT` (AWS S3エンドポイント)
  - `MINIO_ACCESS_KEY`, `MINIO_SECRET_KEY` (AWS IAMキー)
  - `MINIO_BUCKET` (S3バケット名, 例: `title-uploads-devnet`)
- Anchor プログラムがdevnetにデプロイ済み

## アーキテクチャ

```
E2Eテスト (host)
  ├─ TestStorageServer (:7799)    ← signed_json保管用HTTPサーバー
  ├─ SDK → Gateway (:3000)       ← Docker (--network host)
  │         └─ TEE-mock (:4000)  ← Docker (docker compose)
  │             └─ S3            ← AWS S3 (presigned URL経由)
  └─ Solana devnet RPC
```

### 重要: Gateway の起動方法

Gateway は `docker compose` ではなく `docker run --network host` で起動する。

**理由**: `docker compose` でGatewayを起動すると `docker-proxy` のポートバインドが
不安定になり、`port is already allocated` エラーが繰り返し発生する問題がある。
`--network host` はホストのネットワークスタックを直接使うため、この問題を回避できる。

### 重要: S3 vs MinIO

`.env` に AWS S3 の認証情報を設定している場合、ローカルMinIOは不要。
Gateway が presigned URL を生成する際、`.env` の `MINIO_ENDPOINT` / `MINIO_ACCESS_KEY` /
`MINIO_SECRET_KEY` を使用する。MinIOのエンドポイントを上書きしてしまうと、
AWS認証情報とMinIO認証情報の不一致で **403 Forbidden** が発生する。

## 手順

### Step 1: TEE-mock 起動

```bash
cd ~/title-protocol
docker compose up -d tee-mock
# 確認
curl http://localhost:4000/health
# → "ok"
```

### Step 2: Gateway 起動 (--network host)

```bash
# Gatewayイメージのビルド（初回のみ、約5分）
docker compose build gateway

# --network host で起動（.envの設定をそのまま使用）
docker rm -f title-gateway 2>/dev/null
docker run -d --name title-gateway --network host \
  --env-file ~/title-protocol/.env \
  -e TEE_ENDPOINT=http://localhost:4000 \
  title-protocol-gateway

# 確認
sleep 3
curl -s http://localhost:3000/.well-known/title-node-info | jq .
```

`-e TEE_ENDPOINT=http://localhost:4000` のみ上書きする。
`MINIO_*` 系の環境変数は `.env` の値（AWS S3）をそのまま使う。

### Step 3: Global Config 初期化 + Merkle Tree 作成

```bash
cd ~/title-protocol/scripts
source ../.env
node init-config.mjs --rpc "$SOLANA_RPC_URL" --tee http://localhost:4000 --gateway http://localhost:3000
```

成功すると以下が表示される:
- `Tree Address: ...`
- `Signing Pubkey: ...`
- `Merkle Tree 作成完了: ...`

TEE情報は `tests/e2e/fixtures/tee-info.json` に自動保存される。

### Step 4: SDK ビルド

```bash
cd ~/title-protocol/sdk/ts
npm install
npm run build
```

### Step 5: C2PA テストフィクスチャ生成

```bash
cd ~/title-protocol
export OPENSSL_NO_VENDOR=1
cargo run --release --example gen_fixture -p title-core -- tests/e2e/fixtures
```

`OPENSSL_NO_VENDOR=1` はシステムの OpenSSL を使用するための設定。
未設定だと `openssl-sys` がソースからビルドしようとして Perl エラーになる。

生成されるファイル:
- `signed.jpg` — C2PA署名済み画像
- `ingredient_a.jpg`, `ingredient_b.jpg` — 素材画像
- `with_ingredients.jpg` — 素材を含むC2PA署名済み画像

### Step 6: E2E テスト実行

```bash
cd ~/title-protocol/tests/e2e
npm install
npx tsc
source ~/title-protocol/.env
SOLANA_RPC_URL="$SOLANA_RPC_URL" node --test dist/e2e.test.js
```

### 期待される結果

```
▶ E2E Integration Tests
  ✔ Service Health (3 tests)
  ✔ Gateway API (1 test)
  ✔ Verify Flow (E2EE) — C2PA検証 + 暗号化レスポンス復号
  ✔ Sign Flow — /verify → /sign → 部分署名TX取得
  ✔ Provenance Graph — ingredients付き来歴グラフ構築
  ✔ Key Rotation Rejection — 偽TEE鍵の拒否
  ✔ Duplicate Content — 同一コンテンツの冪等性
ℹ tests 9, pass 9, fail 0
```

## テスト内容の詳細

| テスト | 何を検証しているか |
|-------|------------------|
| Service Health (x3) | Gateway node-info応答, Solana RPC接続, TEE情報読み込み |
| Gateway API | presigned URL発行 (`/upload-url`) |
| Verify Flow | E2EE暗号化 → S3アップロード → `/verify` → 復号 → signed_json検証 |
| Sign Flow | `/verify` → signed_json保存 → `/sign` → 部分署名TX取得 |
| Provenance Graph | ingredients付きコンテンツの来歴DAG構築 |
| Key Rotation Rejection | 異なるTEE鍵で署名されたsigned_jsonの拒否（Verify-on-Sign防御） |
| Duplicate Content | 同一コンテンツの2回検証で同一content_hash |

## E2Eフロー図

```
Test Client (host)
  │
  ├─ 1. encryptPayload(tee_pubkey, content)     ← ECDH + HKDF + AES-GCM
  ├─ 2. POST /upload-url → presigned PUT URL
  ├─ 3. PUT presigned URL → S3にアップロード
  ├─ 4. POST /verify { download_url, processor_ids }
  │       └─ Gateway → TEE: C2PA検証 + signed_json生成
  ├─ 5. decryptResponse(symmetric_key, nonce, ciphertext)
  ├─ 6. signed_jsonをTestStorageServerに保存
  ├─ 7. POST /sign { recent_blockhash, signed_json_uri }
  │       └─ Gateway → TEE: Verify-on-Sign + MintV2 TX構築 + 部分署名
  └─ 8. partial_tx検証（信頼TEEノードの署名確認）
```

## トラブルシューティング

### MinIO PUT failed: HTTP 403

`.env` に AWS S3 認証情報が設定されているのに、Gateway起動時に
`-e MINIO_ENDPOINT=http://localhost:9000` 等でローカルMinIOに上書きしていないか確認。
AWS認証情報でローカルMinIOにアクセスすると403になる。

**対策**: Gateway起動時に `MINIO_*` 系を上書きしない。

### port is already allocated

`docker compose` でGatewayを起動しようとすると発生しやすい。

**対策**: `docker run --network host` でGatewayを起動する。
既存コンテナがある場合は `docker rm -f title-gateway` で先に削除。

### openssl-sys: perl エラー

`cargo run` 時に `Can't locate FindBin.pm` エラー。

**対策**: `export OPENSSL_NO_VENDOR=1` を設定してシステムのOpenSSLを使用。

### TEE再起動後にテスト失敗

TEEは起動時に毎回新しい鍵を生成する。再起動後は `init-config.mjs` を
再実行してTree作成 + TEE情報の更新が必要。

### /verify failed: 502

Gateway → TEE の通信失敗。以下を確認:
- TEE-mockが起動しているか: `curl http://localhost:4000/health`
- GatewayのTEE_ENDPOINTが正しいか: `docker inspect title-gateway | grep TEE_ENDPOINT`
