# タスク9: インデクサ実装

## 前提タスク

- タスク5（/sign、cNFT発行）が完了していること（インデクサが検知するcNFTが存在する）

## 読むべきファイル

1. `docs/SPECS_JA.md` — §6.6「インデクサ」
2. `indexer/src/webhook.ts` — 現在のスタブ
3. `indexer/src/poller.ts` — 現在のスタブ
4. `indexer/src/db/schema.ts` — 型定義 + DDLコメント
5. `docker-compose.yml` — PostgreSQLサービス定義

## 作業内容

### DBスキーマ実装

`indexer/src/db/` 配下を整備:

- `migration.ts`: PostgreSQLテーブル作成SQL実行（schema.tsのDDLコメントを実体化）
  - `core_records`: content_hash, owner, cnft_id, signed_json_uri, tee_pubkey, tsa_timestamp, solana_block_time, burnt
  - `extension_records`: content_hash, extension_id, cnft_id, signed_json_uri, wasm_hash, solana_block_time, burnt
- `client.ts`: PostgreSQLクライアント（`pg` パッケージ使用）
  - insert, query by content_hash, mark as burnt 等のCRUD

依存追加: `pg`, `@types/pg`

### Webhookハンドラ

Helius Webhooks（またはカスタムwebhook）からのイベントを処理:

- **MINT**: cNFTのメタデータURIからオフチェーンデータを取得、パースしてDBに挿入
  - Core: content_hash, owner, nodes, links等
  - Extension: content_hash, extension_id, wasm_hash等
- **BURN**: 該当cNFTのrecordをburnt=trueに更新
- **TRANSFER**: ownerを更新

Expressサーバーまたはaxumライクなフレームワーク（`express` が一般的）でHTTPエンドポイントを公開:
`POST /webhook` でイベントを受信。

### DAS APIポーラー

Webhookの欠落を補完:

1. DAS API (`getAssetsByGroup`) でコレクション内のcNFTを定期取得
2. DBに存在しないcNFTを検出して挿入
3. Burn済みcNFTの検出と更新
4. `setInterval` または cron ジョブで定期実行（例: 5分間隔）

### エントリポイント

`indexer/src/index.ts` を更新:
- DB接続初期化
- マイグレーション実行
- Webhookサーバー起動
- ポーラー開始

## 完了条件

- `cd indexer && npm run build` が通る
- PostgreSQL（docker-compose）に対してマイグレーションが成功する
- Webhookハンドラ: MINTイベントを送信 → DBにrecordが挿入される
- Webhookハンドラ: BURNイベントを送信 → recordがburnt=trueになる
- ポーラー: DAS API（モック）からcNFTを取得しDBに同期される
- `docs/COVERAGE.md` の該当箇所を更新
