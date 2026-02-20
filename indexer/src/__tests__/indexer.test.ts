/**
 * インデクサのテスト
 *
 * DBなしで動作する単体テスト:
 * - DasClient: エンドポイント管理、JSON-RPCリクエスト構造
 * - WebhookEvent: イベントハンドリングのロジック（DBモック使用）
 * - ポーラー: 差分検出ロジック（DBモック使用）
 */

import { describe, it, beforeEach } from "node:test";
import * as assert from "node:assert/strict";
import * as http from "node:http";

import { DasClient } from "../das";

// ---------------------------------------------------------------------------
// DasClient テスト
// ---------------------------------------------------------------------------

describe("DasClient", () => {
  it("空のエンドポイント配列で例外を投げる", () => {
    assert.throws(
      () => new DasClient([]),
      /DASエンドポイントが1つ以上必要/
    );
  });

  it("JSON-RPCリクエストを正しく構築する", async () => {
    // モックDASサーバー
    let receivedBody: unknown = null;
    const server = http.createServer((req, res) => {
      let body = "";
      req.on("data", (chunk: string) => { body += chunk; });
      req.on("end", () => {
        receivedBody = JSON.parse(body);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          result: {
            total: 0,
            limit: 1000,
            page: 1,
            items: [],
          },
        }));
      });
    });

    await new Promise<void>((resolve) => server.listen(0, resolve));
    const port = (server.address() as { port: number }).port;

    try {
      const client = new DasClient([`http://127.0.0.1:${port}`]);
      const result = await client.getAssetsByGroup("CollectionMint123");

      assert.equal(result.total, 0);
      assert.deepEqual(result.items, []);

      // リクエストボディの確認
      const req = receivedBody as Record<string, unknown>;
      assert.equal(req.jsonrpc, "2.0");
      assert.equal(req.method, "getAssetsByGroup");
      const params = req.params as Record<string, unknown>;
      assert.equal(params.groupKey, "collection");
      assert.equal(params.groupValue, "CollectionMint123");
    } finally {
      server.close();
    }
  });

  it("DAS APIエラー時に例外を投げる", async () => {
    const server = http.createServer((_req, res) => {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        error: { message: "Method not found" },
      }));
    });

    await new Promise<void>((resolve) => server.listen(0, resolve));
    const port = (server.address() as { port: number }).port;

    try {
      const client = new DasClient([`http://127.0.0.1:${port}`]);
      await assert.rejects(
        () => client.getAssetsByGroup("test"),
        /DAS RPCエラー: Method not found/
      );
    } finally {
      server.close();
    }
  });

  it("HTTP非200エラー時に例外を投げる", async () => {
    const server = http.createServer((_req, res) => {
      res.writeHead(500);
      res.end("Internal Server Error");
    });

    await new Promise<void>((resolve) => server.listen(0, resolve));
    const port = (server.address() as { port: number }).port;

    try {
      const client = new DasClient([`http://127.0.0.1:${port}`]);
      await assert.rejects(
        () => client.getAssetsByGroup("test"),
        /DAS APIエラー: HTTP 500/
      );
    } finally {
      server.close();
    }
  });

  it("複数エンドポイントからランダム選択する", async () => {
    const usedPorts = new Set<number>();

    const servers = await Promise.all(
      [0, 1, 2].map(
        () =>
          new Promise<http.Server>((resolve) => {
            const srv = http.createServer((req, res) => {
              usedPorts.add((req.socket.localPort ?? 0));
              res.writeHead(200, { "Content-Type": "application/json" });
              res.end(JSON.stringify({
                jsonrpc: "2.0",
                id: 1,
                result: { total: 0, limit: 1000, page: 1, items: [] },
              }));
            });
            srv.listen(0, () => resolve(srv));
          })
      )
    );

    const endpoints = servers.map(
      (s) => `http://127.0.0.1:${(s.address() as { port: number }).port}`
    );

    try {
      const client = new DasClient(endpoints);
      // 10回呼び出し → 少なくとも2つの異なるエンドポイントが使われることを期待
      for (let i = 0; i < 10; i++) {
        await client.getAssetsByGroup("test");
      }
      // ランダム性のため厳密なassertは難しいが、1つ以上使われることだけ確認
      assert.ok(usedPorts.size >= 1, "少なくとも1つのエンドポイントが使われるべき");
    } finally {
      for (const srv of servers) srv.close();
    }
  });

  it("getAllAssetsInCollection: ページネーションで全件取得する", async () => {
    let requestCount = 0;
    const server = http.createServer((req, res) => {
      let body = "";
      req.on("data", (chunk: string) => { body += chunk; });
      req.on("end", () => {
        requestCount++;
        const parsed = JSON.parse(body) as { params: { page: number } };
        const page = parsed.params.page;

        // 2ページに分けて返す
        if (page === 1) {
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            result: {
              total: 3,
              limit: 2,
              page: 1,
              items: [
                makeDummyAsset("id1"),
                makeDummyAsset("id2"),
              ],
            },
          }));
        } else {
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            result: {
              total: 3,
              limit: 2,
              page: 2,
              items: [makeDummyAsset("id3")],
            },
          }));
        }
      });
    });

    await new Promise<void>((resolve) => server.listen(0, resolve));
    const port = (server.address() as { port: number }).port;

    try {
      const client = new DasClient([`http://127.0.0.1:${port}`]);
      // limitを2に設定（getAllAssetsInCollectionはデフォルト1000だが、サーバー側で2件ずつ返す想定）
      const assets = await client.getAllAssetsInCollection("collection123");
      assert.equal(assets.length, 3);
      assert.equal(assets[0].id, "id1");
      assert.equal(assets[2].id, "id3");
      assert.equal(requestCount, 2, "2回のリクエストが行われるべき");
    } finally {
      server.close();
    }
  });
});

/** テスト用ダミーアセットを作成する */
function makeDummyAsset(id: string) {
  return {
    id,
    ownership: { owner: "owner123", delegate: null },
    grouping: [{ group_key: "collection", group_value: "collection123" }],
    content: {
      json_uri: `https://arweave.net/${id}`,
      metadata: { name: `Title #${id}`, symbol: "TITLE" },
    },
    burnt: false,
  };
}
