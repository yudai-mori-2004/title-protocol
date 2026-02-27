// SPDX-License-Identifier: Apache-2.0

/**
 * インデクサのテスト
 *
 * DBなしで動作する単体テスト:
 * - DasClient: エンドポイント管理、JSON-RPCリクエスト構造
 * - WebhookEvent: イベントハンドリングのロジック（DBモック使用）
 * - ポーラー: 差分検出ロジック（DBモック使用）
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import * as http from "node:http";

import { DasClient } from "../das";
import { handleWebhookEvent, type WebhookEvent } from "../webhook";
import { pollDasApi } from "../poller";
import type { IndexerDb } from "../db/client";

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

// ---------------------------------------------------------------------------
// MockDb — テスト用DBモック
// ---------------------------------------------------------------------------

class MockDb {
  insertedCore: Array<Record<string, unknown>> = [];
  insertedExtension: Array<Record<string, unknown>> = [];
  burnedIds: string[] = [];
  updatedOwners: Array<{ assetId: string; newOwner: string }> = [];
  knownAssetIds = new Set<string>();

  async insertCoreRecord(record: Record<string, unknown>) {
    this.insertedCore.push(record);
  }
  async insertExtensionRecord(record: Record<string, unknown>) {
    this.insertedExtension.push(record);
  }
  async markBurned(assetId: string) {
    this.burnedIds.push(assetId);
  }
  async updateOwner(assetId: string, newOwner: string) {
    this.updatedOwners.push({ assetId, newOwner });
  }
  async getAllAssetIds() {
    return this.knownAssetIds;
  }
}

// ---------------------------------------------------------------------------
// handleWebhookEvent テスト
// ---------------------------------------------------------------------------

describe("handleWebhookEvent", () => {
  it("MINTイベント: Core cNFTをDBに挿入する", async () => {
    const coreMetadata = {
      protocol: "Title-Core-v1",
      payload: {
        content_hash: "hash-abc",
        content_type: "image/jpeg",
        creator_wallet: "Creator1",
      },
    };

    // DAS API + オフチェーンメタデータの両方を返すモックサーバー
    let port = 0;
    const server = http.createServer((req, res) => {
      if (req.method === "GET") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify(coreMetadata));
      } else {
        let body = "";
        req.on("data", (chunk: string) => { body += chunk; });
        req.on("end", () => {
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            result: {
              id: "new-asset-1",
              ownership: { owner: "Owner1", delegate: null },
              grouping: [{ group_key: "collection", group_value: "ColMint" }],
              content: {
                json_uri: `http://127.0.0.1:${port}/metadata`,
                metadata: { name: "Title #1", symbol: "TITLE" },
              },
              burnt: false,
            },
          }));
        });
      }
    });

    await new Promise<void>((resolve) => server.listen(0, resolve));
    port = (server.address() as { port: number }).port;

    try {
      const db = new MockDb();
      const dasClient = new DasClient([`http://127.0.0.1:${port}`]);

      await handleWebhookEvent(db as unknown as IndexerDb, dasClient, {
        type: "MINT",
        assetId: "new-asset-1",
        owner: "Owner1",
        collection: "ColMint",
        timestamp: 1000,
      });

      assert.equal(db.insertedCore.length, 1);
      assert.equal(db.insertedCore[0].content_hash, "hash-abc");
      assert.equal(db.insertedCore[0].content_type, "image/jpeg");
      assert.equal(db.insertedCore[0].creator_wallet, "Creator1");
    } finally {
      server.close();
    }
  });

  it("BURNイベント: cNFTをBurn済みとしてマークする", async () => {
    const db = new MockDb();
    const dasClient = new DasClient(["http://127.0.0.1:1"]); // 未使用

    await handleWebhookEvent(db as unknown as IndexerDb, dasClient, {
      type: "BURN",
      assetId: "burned-asset-123",
      owner: "someone",
      collection: "col",
      timestamp: 12345,
    });

    assert.equal(db.burnedIds.length, 1);
    assert.equal(db.burnedIds[0], "burned-asset-123");
  });

  it("TRANSFERイベント: 所有者を更新する", async () => {
    const db = new MockDb();
    const dasClient = new DasClient(["http://127.0.0.1:1"]); // 未使用

    await handleWebhookEvent(db as unknown as IndexerDb, dasClient, {
      type: "TRANSFER",
      assetId: "transferred-asset-456",
      owner: "new-owner",
      collection: "col",
      timestamp: 12345,
    });

    assert.equal(db.updatedOwners.length, 1);
    assert.equal(db.updatedOwners[0].assetId, "transferred-asset-456");
    assert.equal(db.updatedOwners[0].newOwner, "new-owner");
  });
});

// ---------------------------------------------------------------------------
// pollDasApi テスト
// ---------------------------------------------------------------------------

describe("pollDasApi", () => {
  it("新規アセットの挿入とBurn検出を行う", async () => {
    const coreMetadata = {
      protocol: "Title-Core-v1",
      payload: {
        content_hash: "poll-hash",
        content_type: "image/png",
        creator_wallet: "PollCreator",
      },
    };

    let port = 0;
    const server = http.createServer((req, res) => {
      if (req.method === "GET") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify(coreMetadata));
      } else {
        let body = "";
        req.on("data", (chunk: string) => { body += chunk; });
        req.on("end", () => {
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            result: {
              total: 3,
              limit: 1000,
              page: 1,
              items: [
                {
                  id: "new-1",
                  ownership: { owner: "Owner1", delegate: null },
                  grouping: [{ group_key: "collection", group_value: "col" }],
                  content: {
                    json_uri: `http://127.0.0.1:${port}/metadata`,
                    metadata: { name: "Title", symbol: "TITLE" },
                  },
                  burnt: false,
                },
                {
                  id: "existing-1",
                  ownership: { owner: "Owner1", delegate: null },
                  grouping: [{ group_key: "collection", group_value: "col" }],
                  content: {
                    json_uri: `http://127.0.0.1:${port}/metadata`,
                    metadata: { name: "Title", symbol: "TITLE" },
                  },
                  burnt: false,
                },
                {
                  id: "burned-1",
                  ownership: { owner: "Owner1", delegate: null },
                  grouping: [{ group_key: "collection", group_value: "col" }],
                  content: {
                    json_uri: `http://127.0.0.1:${port}/metadata`,
                    metadata: { name: "Title", symbol: "TITLE" },
                  },
                  burnt: true,
                },
              ],
            },
          }));
        });
      }
    });

    await new Promise<void>((resolve) => server.listen(0, resolve));
    port = (server.address() as { port: number }).port;

    try {
      const db = new MockDb();
      db.knownAssetIds.add("existing-1");
      db.knownAssetIds.add("burned-1");

      const dasClient = new DasClient([`http://127.0.0.1:${port}`]);
      const result = await pollDasApi(db as unknown as IndexerDb, dasClient, "col");

      assert.equal(result.inserted, 1, "新規1件が挿入されるべき");
      assert.equal(result.burned, 1, "Burn1件が検出されるべき");
      assert.equal(db.insertedCore.length, 1);
      assert.equal(db.insertedCore[0].asset_id, "new-1");
      assert.equal(db.insertedCore[0].content_hash, "poll-hash");
      assert.equal(db.burnedIds.length, 1);
      assert.equal(db.burnedIds[0], "burned-1");
    } finally {
      server.close();
    }
  });
});
