# Task 41: コード監査 — indexer

## 対象
`indexer/` — cNFTインデクサ（TypeScript, PostgreSQL, DAS API）

## ファイル
- `src/index.ts` — エントリポイント、Webhookサーバー、ポーラー起動
- `src/das.ts` — DAS API クライアント（JSON-RPC）
- `src/poller.ts` — DAS API ポーラー（差分同期）
- `src/webhook.ts` — Webhook ハンドラ（MINT/BURN/TRANSFER）
- `src/db/schema.ts` — テーブル定義（TypeScript型 + SQL DDL）
- `src/db/client.ts` — PostgreSQL CRUD
- `src/__tests__/indexer.test.ts` — テスト6件（DasClientのみ）

## 監査で発見された問題

### 不要な依存
1. **`@solana/web3.js` が未使用**:
   `dependencies` に含まれているがソース内で一切importされていない。約2MBの不要な依存。
   → 削除。

### ベンダー中立性
2. **コメント/ドキュメントがHelius固有**:
   コード自体はDAS標準JSON-RPC（getAssetsByGroup, getAsset）で**ベンダー非依存**だが、
   コメントとREADMEが "Helius DAS API" を名指し、例示URLもhelius-rpc.com固定。
   任意のDASプロバイダーで動作する設計なのに、ドキュメントがそれを反映していない。
   → コメント/README/例示URLをベンダー中立に修正。

### テストギャップ
3. **Webhookハンドラのテストがない**:
   MINT/BURN/TRANSFERの3イベント処理が未検証。DBモックで検証可能。
   → テスト追加（+3）。
4. **ポーラーのテストがない**:
   `pollDasApi` の差分検出（新規挿入 + Burn検出）が未検証。
   → テスト追加（+1）。

### 設計メモ（修正不要）
- SQLクエリは全てパラメタライズド — SQLインジェクションリスクなし。
- `ON CONFLICT DO NOTHING` でべき等挿入。ポーラーの並行実行も安全。
- DasClientのランダムエンドポイント選択は堅実。
- WebhookEvent型は簡潔な抽象。DASプロバイダー固有形式の変換は呼び出し側の責務で正しい。
- クエリAPIは意図的に未定義（デプロイ先に応じて別層で提供）。

## 完了基準
- [x] `@solana/web3.js` を依存から削除
- [x] Helius固有参照をベンダー中立に修正（コメント + README + 例示URL）
- [x] Webhookハンドラテスト追加（MINT/BURN/TRANSFER: +3）
- [x] ポーラーテスト追加（pollDasApi: +1）
- [x] `npm run build` パス
- [x] `npm test` パス（10テスト: 既存6 + 新規4）

## 対処内容

### 1. `@solana/web3.js` 削除
- ソース内で一切importされていない未使用依存を `package.json` から除去。

### 2. ベンダー中立化
- `das.ts`: "Helius DAS API" → "DAS (Digital Asset Standard) API" + ベンダー中立な例示URL
- `webhook.ts`: "Helius Webhooks等のサービス" → "DASプロバイダーのWebhook機能"
- `index.ts`: 例示URLをベンダー中立に
- `README.md`: Helius docsリンク → Solana公式ガイドリンク + 例示URLをベンダー中立に

### 3. Webhookハンドラテスト追加（+3テスト）
- `MINTイベント: Core cNFTをDBに挿入する` — DAS APIモック + メタデータモック + DBモックで、MINTイベントがCore cNFTレコードとしてDBに挿入されることを確認
- `BURNイベント: cNFTをBurn済みとしてマークする` — markBurnedが呼ばれることを確認
- `TRANSFERイベント: 所有者を更新する` — updateOwnerが正しいassetIdとnewOwnerで呼ばれることを確認

### 4. ポーラーテスト追加（+1テスト）
- `新規アセットの挿入とBurn検出を行う` — DAS APIが3件（新規/既存/Burn済み）を返す状況で、新規1件の挿入とBurn1件の検出を確認
