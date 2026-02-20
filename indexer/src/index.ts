/**
 * Title Protocol cNFT インデクサ
 *
 * 仕様書 §6.6: cNFTのインデックスを構築し、
 * 複雑なクエリに対応した柔軟な検索を可能にする。
 *
 * ## 環境変数
 * - DATABASE_URL: PostgreSQL接続文字列
 * - DAS_ENDPOINTS: カンマ区切りのDAS APIエンドポイント（APIキー付きURL）
 *   例: "https://mainnet.helius-rpc.com/?api-key=xxx,https://devnet.helius-rpc.com/?api-key=yyy"
 * - COLLECTION_MINTS: カンマ区切りの監視対象コレクションMintアドレス
 * - POLL_INTERVAL_MS: ポーリング間隔（ミリ秒、デフォルト: 300000 = 5分）
 * - WEBHOOK_PORT: Webhookサーバーのポート（デフォルト: 5000）
 */

import * as http from "node:http";
import { IndexerDb } from "./db/client";
import { DasClient } from "./das";
import { handleWebhookEvent, type WebhookEvent } from "./webhook";
import { startPoller } from "./poller";

export { IndexerDb } from "./db/client";
export { DasClient } from "./das";
export type { DasAsset, DasGetAssetsByGroupResponse } from "./das";
export { handleWebhookEvent, type WebhookEvent } from "./webhook";
export { pollDasApi, startPoller } from "./poller";
export type { CoreRecord, ExtensionRecord } from "./db/schema";

/**
 * インデクサのメインエントリポイント。
 * DB接続 → マイグレーション → Webhookサーバー起動 → ポーラー開始
 */
async function main(): Promise<void> {
  // 1. 設定読み込み
  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    console.error("DATABASE_URL環境変数が設定されていません");
    process.exit(1);
  }

  const dasEndpointsRaw = process.env.DAS_ENDPOINTS ?? "";
  const dasEndpoints = dasEndpointsRaw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  if (dasEndpoints.length === 0) {
    console.error("DAS_ENDPOINTS環境変数が設定されていません");
    process.exit(1);
  }

  const collectionMintsRaw = process.env.COLLECTION_MINTS ?? "";
  const collectionMints = collectionMintsRaw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  if (collectionMints.length === 0) {
    console.error("COLLECTION_MINTS環境変数が設定されていません");
    process.exit(1);
  }

  const pollIntervalMs = parseInt(process.env.POLL_INTERVAL_MS ?? "300000", 10);
  const webhookPort = parseInt(process.env.WEBHOOK_PORT ?? "5000", 10);

  // 2. DB接続 + マイグレーション
  const db = new IndexerDb(databaseUrl);
  console.log("[indexer] マイグレーション実行中...");
  await db.migrate();
  console.log("[indexer] マイグレーション完了");

  // 3. DASクライアント初期化
  const dasClient = new DasClient(dasEndpoints);
  console.log(`[indexer] DASエンドポイント: ${dasEndpoints.length}個`);

  // 4. Webhookサーバー起動
  const server = http.createServer(async (req, res) => {
    if (req.method === "POST" && req.url === "/webhook") {
      let body = "";
      req.on("data", (chunk: string) => {
        body += chunk;
      });
      req.on("end", async () => {
        try {
          const events = JSON.parse(body) as WebhookEvent | WebhookEvent[];
          const eventArray = Array.isArray(events) ? events : [events];

          for (const event of eventArray) {
            await handleWebhookEvent(db, dasClient, event);
          }

          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ ok: true }));
        } catch (err) {
          console.error("[webhook] エラー:", err);
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: String(err) }));
        }
      });
    } else if (req.method === "GET" && req.url === "/health") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ status: "ok" }));
    } else {
      res.writeHead(404);
      res.end("Not Found");
    }
  });

  server.listen(webhookPort, () => {
    console.log(`[indexer] Webhookサーバー起動: port ${webhookPort}`);
  });

  // 5. ポーラー開始
  console.log(
    `[indexer] ポーラー開始: ${collectionMints.length}コレクション, 間隔${pollIntervalMs}ms`
  );
  startPoller(db, dasClient, collectionMints, pollIntervalMs);
}

// エントリポイント（直接実行時のみ）
if (require.main === module) {
  main().catch((err) => {
    console.error("[indexer] 致命的エラー:", err);
    process.exit(1);
  });
}
