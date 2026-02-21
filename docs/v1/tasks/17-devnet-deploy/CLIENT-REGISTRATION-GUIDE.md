# クライアントからのC2PAコンテンツ登録ガイド

Devnet環境上のGateway/TEEに対して、ローカル環境（クライアント）から任意のC2PA署名済みコンテンツを登録する手順。

## 前提条件

- **Node.js**: v20以上
- **Solana CLI**: devnet設定済み、ウォレット (`~/.config/solana/id.json`) が存在しSOLがあること。
- **SDKのビルド**: `sdk/ts` ディレクトリで `npm install && npm run build` が完了していること。
- **SSH接続**: EC2サーバーへのSSH接続が可能であること（TEE情報更新のため）。
- **EC2サーバー**: すでにGatewayとTEEが起動していること。

## 手順

### 1. 環境変数の設定

GatewayのエンドポイントとSolana RPCを設定します。

```bash
export GATEWAY_URL=http://<EC2_PUBLIC_IP>:3000
export SOLANA_RPC_URL=https://api.devnet.solana.com
# または Helius 等の専用RPC
```

### 2. TEE接続情報の更新 (重要)

TEEが再起動された場合や、クライアントの `tee-info.json` が古い場合は、最新のTEE暗号化鍵を取得する必要があります。
`init-config.mjs` をSSHポートフォワード経由で実行するのが最も確実です。これにより以下が自動実行されます：
1. TEEの公開鍵情報をGlobal Configに登録
2. TEE内部ウォレットへのSOL送金
3. Merkle Treeの作成（初回のみ）
4. ローカルへの `tee-info.json` 保存

```bash
# 1. SSHポートフォワードを開始 (バックグラウンド)
# ローカルの 3000, 4000 ポートをリモートに転送
ssh -f -N -L 4000:localhost:4000 -L 3000:localhost:3000 -i deploy/keys/title-protocol-devnet.pem -o StrictHostKeyChecking=no ec2-user@<EC2_PUBLIC_IP>

# 2. init-config.mjs を実行
# --tee と --gateway は localhost (転送先) を指定
node scripts/init-config.mjs --rpc "$SOLANA_RPC_URL" --tee http://localhost:4000 --gateway http://localhost:3000
```

成功すると `tests/e2e/fixtures/tee-info.json` が更新されます。
完了したらSSHポートフォワードは終了して構いませんが、Gatewayへのアクセスを `localhost:3000` 経由で行う場合は維持してください。

### 3. コンテンツ登録の実行

指定した画像を登録し、権利トークン（Core）とExtensionトークンを取得します。

**注意: S3バケットの設定**
Devnet環境では、登録されたメタデータ（signed_json）へのURLがオンチェーンに記録されます。
S3の署名付きURLは非常に長く、SolanaのURI長制限（`MetadataUriTooLong`）を超えるため、クエリパラメータを除去した**短縮URL（パブリックURL）**を使用する必要があります。
そのため、サーバー側のS3バケット（`title-uploads-devnet`）は**パブリック読み取り可能**（Bucket Policyで `s3:GetObject` を `Principal: *` に許可）である必要があります。

```bash
# Gatewayに直接アクセスする場合 (ポートフォワード終了後)
export GATEWAY_URL=http://<EC2_PUBLIC_IP>:3000

# 画像を登録 (Core + pHash)
# register-content.mjs はURL短縮ロジックを含む修正版を使用すること
node scripts/register-content.mjs <PATH_TO_IMAGE> --processor core-c2pa,phash-v1
```

成功すると、Solana Explorerへのトランザクションリンクが表示されます。

## 実績とトラブルシューティング (2026-02-21)

Google Pixel 10で撮影された画像 (`PXL_20251216_122821334.jpg`) の登録において、以下の問題に対処しました。

### 1. Active Manifestが見つかりません
**原因**: サーバー側の `c2pa` クレートが `0.47` であり、Pixelの `C2PA v2` フォーマット（descriptionフィールドを含む）に対応していなかった。
**対策**: `Cargo.toml` で `c2pa = "0.75"` に更新し、Gateway/TEEを再ビルド・再デプロイした。

### 2. WASMバイナリが見つからない
**原因**: TEEコンテナ内に `/wasm-modules/` が存在しなかった。
**対策**: EC2上で `docker cp` を使用してホストからコンテナへWASMモジュールをコピーした。
```bash
docker exec title-protocol-tee-mock-1 mkdir -p /wasm-modules
docker cp ~/title-protocol/wasm-modules/. title-protocol-tee-mock-1:/wasm-modules/
```

### 3. MetadataUriTooLong エラー
**原因**: S3の署名付きURLが長すぎて、Bubblegumの `MintV2` インストラクションに入りきらなかった。
**対策**: `scripts/register-content.mjs` を修正し、`download_url` からクエリパラメータ（署名）を除去して記録するようにした。これに伴い、S3バケットをパブリック公開設定に変更した。

### 4. HTTP 403 Forbidden
**原因**: S3バケットをパブリックにする際、Block Public Access設定は解除したが、Bucket Policyが適用されていなかった。
**対策**: AWSコンソールからバケットポリシーを追加し、`s3:GetObject` を全ユーザーに許可した。
