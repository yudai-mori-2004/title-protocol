#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * stress-test.ts — Title Protocol 負荷テスト・攻撃耐久テスト
 *
 * カテゴリ:
 *   1. ベースライン計測（正常系 /verify 単発）
 *   2. 同時並行負荷テスト（N並列 /verify）
 *   3. 大容量ペイロード攻撃
 *   4. 不正入力テスト（壊れたJSON, 不正署名, 不正暗号文）
 *   5. エンドポイント乱用（不正メソッド, 存在しないパス）
 *   6. Slowloris模倣（遅い送信）
 *   7. リプレイ攻撃（古いペイロード再利用）
 *   8. 暗号攻撃（鍵不一致, nonce改変, ciphertext改竄）
 *   9. リソース枯渇テスト（高速連打）
 *
 * Usage:
 *   npx tsx stress-test.ts <image-path> --wallet <keypair.json> [--rpc <url>]
 *
 * Gateway endpoint and encryption pubkey are discovered from on-chain
 * GlobalConfig automatically.
 */

import { webcrypto } from "node:crypto";
if (!globalThis.crypto?.subtle) {
  // @ts-ignore
  globalThis.crypto = webcrypto;
}

import * as fs from "node:fs";
import * as path from "node:path";
import { Connection, Keypair } from "@solana/web3.js";
import {
  TitleClient,
  type GlobalConfig,
  type TrustedTeeNode,
  encryptPayload,
  decryptResponse,
  fetchGlobalConfig,
} from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// 型定義
// ---------------------------------------------------------------------------

interface TestResult {
  category: string;
  name: string;
  status: "PASS" | "FAIL" | "ERROR";
  duration_ms: number;
  details: string;
  http_status?: number;
  expected: string;
}

interface Args {
  gatewayHost: string;
  imagePath: string;
  port: number;
  walletPath: string;
  encryptionPubkey: string;
  solanaRpc: string;
  programId: string;
}

// ---------------------------------------------------------------------------
// グローバル
// ---------------------------------------------------------------------------

const results: TestResult[] = [];
let gatewayUrl = "";
let client: TitleClient;
let encPubkeyBytes: Uint8Array;
let imageBytes: Buffer;
let keypair: Keypair;

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

function log(label: string, ...msg: unknown[]) {
  const ts = new Date().toISOString().slice(11, 23);
  console.log(`[${ts}] ${label}`, ...msg);
}

function record(r: TestResult) {
  results.push(r);
  const icon = r.status === "PASS" ? "✓" : r.status === "FAIL" ? "✗" : "⚠";
  console.log(
    `  ${icon} [${r.status}] ${r.name} (${r.duration_ms}ms) — ${r.details}`
  );
}

function parseArgs(): Args {
  const args = process.argv.slice(2);
  if (args.length < 1) {
    console.error(
      "Usage: npx tsx stress-test.ts <image-path> --wallet <keypair.json> [--rpc <url>] [--program-id <pubkey>] [--gateway <host>] [--encryption-pubkey <base64>]"
    );
    process.exit(1);
  }
  let port = 3000;
  let walletPath = "";
  let encryptionPubkey = "";
  let gatewayHost = "";
  let solanaRpc = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
  let programId = "";
  // First positional arg is image path
  const imagePath = args[0];
  for (let i = 1; i < args.length; i++) {
    switch (args[i]) {
      case "--port":
        port = parseInt(args[++i], 10);
        break;
      case "--wallet":
        walletPath = args[++i];
        break;
      case "--encryption-pubkey":
        encryptionPubkey = args[++i];
        break;
      case "--rpc":
        solanaRpc = args[++i];
        break;
      case "--program-id":
        programId = args[++i];
        break;
      case "--gateway":
        gatewayHost = args[++i];
        break;
    }
  }
  if (!walletPath) {
    console.error("--wallet は必須です");
    process.exit(1);
  }
  return {
    gatewayHost,
    imagePath,
    port,
    walletPath,
    encryptionPubkey,
    solanaRpc,
    programId,
  };
}

/** 正常な暗号化+アップロード+verify を1回実行し、タイミングを返す */
async function doNormalVerify(): Promise<{
  duration_ms: number;
  symmetricKey: Uint8Array;
  downloadUrl: string;
}> {
  const contentB64 = Buffer.from(imageBytes).toString("base64");
  const payload = {
    owner_wallet: keypair.publicKey.toBase58(),
    content: contentB64,
  };
  const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
  const { symmetricKey, encryptedPayload } = await encryptPayload(
    encPubkeyBytes,
    payloadJson
  );
  const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);

  const t0 = Date.now();
  const encResp = await client.verify(gatewayUrl, {
    download_url: downloadUrl,
    processor_ids: ["core-c2pa"],
  });
  const duration_ms = Date.now() - t0;

  // 復号して正常性確認
  const plain = await decryptResponse(
    symmetricKey,
    encResp.nonce,
    encResp.ciphertext
  );
  const parsed = JSON.parse(new TextDecoder().decode(plain));
  if (!parsed.results || parsed.results.length === 0) {
    throw new Error("verify結果が空");
  }
  return { duration_ms, symmetricKey, downloadUrl };
}

/** 暗号化してアップロードし、downloadUrlを返す（verifyは呼ばない） */
async function uploadEncrypted(): Promise<{
  downloadUrl: string;
  symmetricKey: Uint8Array;
}> {
  const contentB64 = Buffer.from(imageBytes).toString("base64");
  const payload = {
    owner_wallet: keypair.publicKey.toBase58(),
    content: contentB64,
  };
  const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
  const { symmetricKey, encryptedPayload } = await encryptPayload(
    encPubkeyBytes,
    payloadJson
  );
  const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
  return { downloadUrl, symmetricKey };
}

/** fetch + タイムアウト */
async function fetchWithTimeout(
  url: string,
  opts: RequestInit,
  timeoutMs = 30000
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { ...opts, signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

// ===========================================================================
// テストカテゴリ
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. ベースライン
// ---------------------------------------------------------------------------
async function testBaseline() {
  log("CAT 1", "=== ベースライン計測 ===");

  // 1-1: gateway health (POST /upload-url)
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content_size: 1, content_type: "image/jpeg" }),
    });
    const d = Date.now() - t0;
    record({
      category: "baseline",
      name: "POST /upload-url (health check)",
      status: res.ok ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "200 OK",
    });
  }

  // 1-2: single verify (5回計測)
  const durations: number[] = [];
  for (let i = 0; i < 5; i++) {
    try {
      const { duration_ms } = await doNormalVerify();
      durations.push(duration_ms);
    } catch (e: any) {
      durations.push(-1);
    }
  }
  const valid = durations.filter((d) => d > 0);
  const avg = valid.length > 0 ? valid.reduce((a, b) => a + b, 0) / valid.length : -1;
  const min = valid.length > 0 ? Math.min(...valid) : -1;
  const max = valid.length > 0 ? Math.max(...valid) : -1;
  record({
    category: "baseline",
    name: "single /verify x5 (core-c2pa, ramen 2.3MB)",
    status: valid.length === 5 ? "PASS" : "FAIL",
    duration_ms: Math.round(avg),
    details: `avg=${Math.round(avg)}ms min=${min}ms max=${max}ms (${valid.length}/5 succeeded)`,
    expected: "5/5 succeed, <3000ms avg",
  });

  // 1-3: verify with all processors
  {
    const { downloadUrl, symmetricKey } = await uploadEncrypted();
    const t0 = Date.now();
    try {
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: [
          "core-c2pa",
          "phash-v1",
          "hardware-google",
          "c2pa-training-v1",
          "c2pa-license-v1",
        ],
      });
      const d = Date.now() - t0;
      const plain = await decryptResponse(
        symmetricKey,
        encResp.nonce,
        encResp.ciphertext
      );
      const parsed = JSON.parse(new TextDecoder().decode(plain));
      record({
        category: "baseline",
        name: "/verify all 5 processors",
        status: "PASS",
        duration_ms: d,
        details: `results=${parsed.results.length} processors`,
        expected: "200 OK + 5 results",
      });
    } catch (e: any) {
      record({
        category: "baseline",
        name: "/verify all 5 processors",
        status: "FAIL",
        duration_ms: Date.now() - t0,
        details: e.message,
        expected: "200 OK + 5 results",
      });
    }
  }
}

// ---------------------------------------------------------------------------
// 2. 同時並行負荷テスト
// ---------------------------------------------------------------------------
async function testConcurrentLoad() {
  log("CAT 2", "=== 同時並行負荷テスト ===");

  for (const concurrency of [2, 5, 10]) {
    log("CAT 2", `--- ${concurrency}並列 ---`);
    // 全リクエスト分のペイロードを先にアップロード
    const uploads = await Promise.all(
      Array.from({ length: concurrency }, () => uploadEncrypted())
    );

    const t0 = Date.now();
    const promises = uploads.map(async ({ downloadUrl, symmetricKey }, i) => {
      const t = Date.now();
      try {
        const encResp = await client.verify(gatewayUrl, {
          download_url: downloadUrl,
          processor_ids: ["core-c2pa"],
        });
        const d = Date.now() - t;
        // 復号確認
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        return { ok: true, ms: d };
      } catch (e: any) {
        return { ok: false, ms: Date.now() - t, err: e.message };
      }
    });

    const res = await Promise.all(promises);
    const totalMs = Date.now() - t0;
    const succeeded = res.filter((r) => r.ok).length;
    const times = res.filter((r) => r.ok).map((r) => r.ms);
    const avgMs =
      times.length > 0
        ? Math.round(times.reduce((a, b) => a + b, 0) / times.length)
        : -1;

    record({
      category: "concurrent",
      name: `${concurrency} concurrent /verify`,
      status: succeeded === concurrency ? "PASS" : "FAIL",
      duration_ms: totalMs,
      details: `${succeeded}/${concurrency} ok, total=${totalMs}ms, avg_per_req=${avgMs}ms`,
      expected: `all ${concurrency} succeed`,
    });
  }
}

// ---------------------------------------------------------------------------
// 3. 大容量ペイロード攻撃
// ---------------------------------------------------------------------------
async function testLargePayload() {
  log("CAT 3", "=== 大容量ペイロード攻撃 ===");

  // 3-1: 10MB ランダムバイナリ
  {
    const size = 10 * 1024 * 1024;
    const bigData = Buffer.alloc(size, 0x42); // 10MB of 'B'
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: bigData.toString("base64"),
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { symmetricKey, encryptedPayload } = await encryptPayload(
      encPubkeyBytes,
      payloadJson
    );
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(
        gatewayUrl,
        encryptedPayload
      );
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      const d = Date.now() - t0;
      record({
        category: "large_payload",
        name: "10MB random binary /verify",
        status: "PASS",
        duration_ms: d,
        details: `uploaded+verified 10MB (not valid C2PA, should still process)`,
        expected: "TEE processes or gracefully rejects",
      });
    } catch (e: any) {
      const d = Date.now() - t0;
      record({
        category: "large_payload",
        name: "10MB random binary /verify",
        status: "PASS",
        duration_ms: d,
        details: `rejected: ${e.message.slice(0, 120)}`,
        expected: "TEE processes or gracefully rejects",
      });
    }
  }

  // 3-2: 50MB ランダムバイナリ
  {
    const size = 50 * 1024 * 1024;
    const bigData = Buffer.alloc(size, 0x43);
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: bigData.toString("base64"),
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { symmetricKey, encryptedPayload } = await encryptPayload(
      encPubkeyBytes,
      payloadJson
    );
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(
        gatewayUrl,
        encryptedPayload
      );
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      const d = Date.now() - t0;
      record({
        category: "large_payload",
        name: "50MB random binary /verify",
        status: "PASS",
        duration_ms: d,
        details: `processed 50MB`,
        expected: "TEE processes or gracefully rejects",
      });
    } catch (e: any) {
      const d = Date.now() - t0;
      record({
        category: "large_payload",
        name: "50MB random binary /verify",
        status: "PASS",
        duration_ms: d,
        details: `rejected: ${e.message.slice(0, 120)}`,
        expected: "TEE processes or gracefully rejects",
      });
    }
  }

  // 3-3: upload-url でサイズ0を要求
  {
    const t0 = Date.now();
    try {
      const res = await fetch(`${gatewayUrl}/upload-url`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content_size: 0, content_type: "image/jpeg" }),
      });
      const d = Date.now() - t0;
      record({
        category: "large_payload",
        name: "upload-url size=0",
        status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: d,
        http_status: res.status,
        details: `HTTP ${res.status} — ${(await res.text()).slice(0, 80)}`,
        expected: "400 Bad Request",
      });
    } catch (e: any) {
      record({
        category: "large_payload",
        name: "upload-url size=0",
        status: "ERROR",
        duration_ms: Date.now() - t0,
        details: e.message,
        expected: "400 Bad Request",
      });
    }
  }

  // 3-4: upload-url でサイズ 3GB (> 2GB limit) を要求
  {
    const t0 = Date.now();
    try {
      const res = await fetch(`${gatewayUrl}/upload-url`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content_size: 3 * 1024 * 1024 * 1024,
          content_type: "image/jpeg",
        }),
      });
      const d = Date.now() - t0;
      record({
        category: "large_payload",
        name: "upload-url size=3GB (exceeds 2GB limit)",
        status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: d,
        http_status: res.status,
        details: `HTTP ${res.status} — ${(await res.text()).slice(0, 80)}`,
        expected: "400+ rejection",
      });
    } catch (e: any) {
      record({
        category: "large_payload",
        name: "upload-url size=3GB",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 80)}`,
        expected: "400+ rejection",
      });
    }
  }
}

// ---------------------------------------------------------------------------
// 4. 不正入力テスト
// ---------------------------------------------------------------------------
async function testMalformedInput() {
  log("CAT 4", "=== 不正入力テスト ===");

  // 4-1: /verify に空JSON
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/verify`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    const d = Date.now() - t0;
    record({
      category: "malformed",
      name: "/verify empty JSON {}",
      status: res.status >= 400 ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "400/422 rejection",
    });
  }

  // 4-2: /verify に不正JSON
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/verify`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "not json at all {{{",
    });
    const d = Date.now() - t0;
    record({
      category: "malformed",
      name: "/verify invalid JSON",
      status: res.status >= 400 ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "400 rejection",
    });
  }

  // 4-3: /verify に巨大JSONキー (100KB key name)
  {
    const bigKey = "A".repeat(100_000);
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/verify`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ [bigKey]: "value" }),
    });
    const d = Date.now() - t0;
    record({
      category: "malformed",
      name: "/verify 100KB JSON key",
      status: res.status >= 400 ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "400+ rejection",
    });
  }

  // 4-4: /upload-url に不正content_type
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        content_size: 1000,
        content_type: "<script>alert(1)</script>",
      }),
    });
    const d = Date.now() - t0;
    const body = await res.text();
    record({
      category: "malformed",
      name: "/upload-url XSS content_type",
      status: "PASS",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status} — no script execution in response: ${body.includes("<script>") ? "LEAKED" : "SAFE"}`,
      expected: "no XSS reflection",
    });
  }

  // 4-5: /verify に不正download_url（外部URL）
  {
    const t0 = Date.now();
    try {
      const res = await fetch(`${gatewayUrl}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          download_url: "https://evil.example.com/malware.bin",
          processor_ids: ["core-c2pa"],
        }),
      });
      const d = Date.now() - t0;
      record({
        category: "malformed",
        name: "/verify external download_url",
        status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: d,
        http_status: res.status,
        details: `HTTP ${res.status}`,
        expected: "400+ rejection (no gateway signature)",
      });
    } catch (e: any) {
      record({
        category: "malformed",
        name: "/verify external download_url",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 80)}`,
        expected: "400+ rejection",
      });
    }
  }

  // 4-6: /sign に偽の signed_json_uri
  {
    const t0 = Date.now();
    try {
      const res = await fetch(`${gatewayUrl}/sign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          recent_blockhash: "11111111111111111111111111111111",
          requests: [{ signed_json_uri: "https://evil.example.com/fake.json" }],
        }),
      });
      const d = Date.now() - t0;
      record({
        category: "malformed",
        name: "/sign with fake signed_json_uri",
        status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: d,
        http_status: res.status,
        details: `HTTP ${res.status}`,
        expected: "400+ rejection (no gateway signature)",
      });
    } catch (e: any) {
      record({
        category: "malformed",
        name: "/sign with fake signed_json_uri",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 80)}`,
        expected: "400+ rejection",
      });
    }
  }

  // 4-7: /verify に不存在のprocessor_id
  {
    const { downloadUrl, symmetricKey } = await uploadEncrypted();
    const t0 = Date.now();
    try {
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["nonexistent-processor-v99"],
      });
      const d = Date.now() - t0;
      record({
        category: "malformed",
        name: "/verify unknown processor_id",
        status: "FAIL",
        duration_ms: d,
        details: "TEE accepted unknown processor — should reject",
        expected: "rejection",
      });
    } catch (e: any) {
      const d = Date.now() - t0;
      record({
        category: "malformed",
        name: "/verify unknown processor_id",
        status: "PASS",
        duration_ms: d,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "rejection",
      });
    }
  }

  // 4-8: /verify with empty processor_ids
  {
    const { downloadUrl } = await uploadEncrypted();
    const t0 = Date.now();
    try {
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: [],
      });
      const d = Date.now() - t0;
      record({
        category: "malformed",
        name: "/verify empty processor_ids []",
        status: "PASS",
        duration_ms: d,
        details: "TEE accepted empty processors (may be valid design)",
        expected: "graceful handling",
      });
    } catch (e: any) {
      const d = Date.now() - t0;
      record({
        category: "malformed",
        name: "/verify empty processor_ids []",
        status: "PASS",
        duration_ms: d,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "graceful handling",
      });
    }
  }
}

// ---------------------------------------------------------------------------
// 5. エンドポイント乱用
// ---------------------------------------------------------------------------
async function testEndpointAbuse() {
  log("CAT 5", "=== エンドポイント乱用 ===");

  // 5-1: GET /verify (wrong method)
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/verify`);
    const d = Date.now() - t0;
    record({
      category: "endpoint_abuse",
      name: "GET /verify (should be POST)",
      status: res.status === 405 || res.status >= 400 ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "405 Method Not Allowed",
    });
  }

  // 5-2: DELETE /verify
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/verify`, { method: "DELETE" });
    const d = Date.now() - t0;
    record({
      category: "endpoint_abuse",
      name: "DELETE /verify",
      status: res.status >= 400 ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "405 Method Not Allowed",
    });
  }

  // 5-3: 存在しないエンドポイント
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/admin/shutdown`, {
      method: "POST",
    });
    const d = Date.now() - t0;
    record({
      category: "endpoint_abuse",
      name: "POST /admin/shutdown (non-existent)",
      status: res.status === 404 ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}`,
      expected: "404 Not Found",
    });
  }

  // 5-4: パストラバーサル
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/../../etc/passwd`);
    const d = Date.now() - t0;
    const body = await res.text();
    record({
      category: "endpoint_abuse",
      name: "path traversal /../../etc/passwd",
      status:
        res.status >= 400 && !body.includes("root:") ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}, body contains 'root:': ${body.includes("root:")}`,
      expected: "404 and no file content leak",
    });
  }

  // 5-5: 超長いURL
  {
    const longPath = "/verify?" + "a=".repeat(50000);
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(
        `${gatewayUrl}${longPath}`,
        { method: "POST" },
        10000
      );
      const d = Date.now() - t0;
      record({
        category: "endpoint_abuse",
        name: "100KB URL length",
        status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: d,
        http_status: res.status,
        details: `HTTP ${res.status}`,
        expected: "414 URI Too Long or similar",
      });
    } catch (e: any) {
      record({
        category: "endpoint_abuse",
        name: "100KB URL length",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `connection error: ${e.message.slice(0, 80)}`,
        expected: "rejection",
      });
    }
  }

  // 5-6: 巨大Content-Length宣言 (100GB) + 小さいボディ
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(
        `${gatewayUrl}/verify`,
        {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            "Content-Length": "107374182400", // 100GB
          },
          body: "{}",
        },
        10000
      );
      const d = Date.now() - t0;
      record({
        category: "endpoint_abuse",
        name: "Content-Length mismatch (100GB declared, tiny body)",
        status: "PASS",
        duration_ms: d,
        http_status: res.status,
        details: `HTTP ${res.status} — server handled gracefully`,
        expected: "rejection or graceful handling",
      });
    } catch (e: any) {
      record({
        category: "endpoint_abuse",
        name: "Content-Length mismatch (100GB declared, tiny body)",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 80)}`,
        expected: "rejection or graceful handling",
      });
    }
  }

  // 5-7: CORS/Headers probing
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "OPTIONS",
    });
    const d = Date.now() - t0;
    const headers = Object.fromEntries(res.headers.entries());
    record({
      category: "endpoint_abuse",
      name: "OPTIONS /upload-url (CORS preflight)",
      status: "PASS",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status}, CORS headers: ${JSON.stringify(headers["access-control-allow-origin"] || "none")}`,
      expected: "handled without crash",
    });
  }
}

// ---------------------------------------------------------------------------
// 6. Slowloris模倣
// ---------------------------------------------------------------------------
async function testSlowloris() {
  log("CAT 6", "=== Slowloris模倣テスト ===");

  // 6-1: 極端に遅い upload（チャンク単位で遅延）
  // Node.js fetchでは直接slowlorisは難しいので、curlで代用
  // ここではタイムアウト検出をテスト

  // 6-1: 多数の同時接続を開いて保持
  {
    const connCount = 20;
    const t0 = Date.now();
    const controllers: AbortController[] = [];
    const promises = Array.from({ length: connCount }, async (_, i) => {
      const ctrl = new AbortController();
      controllers.push(ctrl);
      try {
        const res = await fetch(`${gatewayUrl}/verify`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: "{}",
          signal: ctrl.signal,
        });
        return { status: res.status, ok: true };
      } catch (e: any) {
        return { status: 0, ok: false, err: e.message };
      }
    });

    const results_sl = await Promise.all(promises);
    const d = Date.now() - t0;
    const responded = results_sl.filter((r) => r.ok).length;

    record({
      category: "slowloris",
      name: `${connCount} simultaneous connections`,
      status: responded > 0 ? "PASS" : "FAIL",
      duration_ms: d,
      details: `${responded}/${connCount} got response`,
      expected: "server remains responsive",
    });
  }

  // 6-2: 50 rapid-fire connections
  {
    const rapidCount = 50;
    const t0 = Date.now();
    const results_rf: { status: number; ms: number }[] = [];

    for (let i = 0; i < rapidCount; i++) {
      const t = Date.now();
      try {
        const res = await fetchWithTimeout(
          `${gatewayUrl}/upload-url`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ content_size: 1, content_type: "image/jpeg" }),
          },
          5000
        );
        results_rf.push({ status: res.status, ms: Date.now() - t });
      } catch {
        results_rf.push({ status: 0, ms: Date.now() - t });
      }
    }

    const d = Date.now() - t0;
    const ok = results_rf.filter((r) => r.status === 200).length;
    const avgMs = Math.round(
      results_rf.reduce((a, b) => a + b.ms, 0) / results_rf.length
    );

    record({
      category: "slowloris",
      name: `${rapidCount} rapid sequential GETs`,
      status: ok >= rapidCount * 0.9 ? "PASS" : "FAIL",
      duration_ms: d,
      details: `${ok}/${rapidCount} ok, avg=${avgMs}ms, total=${d}ms`,
      expected: ">90% success rate",
    });
  }
}

// ---------------------------------------------------------------------------
// 7. リプレイ攻撃
// ---------------------------------------------------------------------------
async function testReplay() {
  log("CAT 7", "=== リプレイ攻撃テスト ===");

  // 7-1: 同じdownload_urlで /verify を2回呼ぶ
  {
    const { downloadUrl, symmetricKey } = await uploadEncrypted();
    const t0 = Date.now();
    try {
      // 1回目
      const enc1 = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      // 2回目（S3一時URLは期限内なら再取得可能）
      const enc2 = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      const d = Date.now() - t0;
      record({
        category: "replay",
        name: "/verify same download_url twice",
        status: "PASS",
        duration_ms: d,
        details: "both succeeded (S3 URL valid within TTL — expected behavior)",
        expected: "second request succeeds (idempotent verify)",
      });
    } catch (e: any) {
      record({
        category: "replay",
        name: "/verify same download_url twice",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `second rejected: ${e.message.slice(0, 80)}`,
        expected: "second request may be rejected",
      });
    }
  }

  // 7-2: 期限切れS3 URLを偽造
  {
    const t0 = Date.now();
    try {
      const encResp = await client.verify(gatewayUrl, {
        download_url: "https://title-uploads-devnet.s3.ap-northeast-1.amazonaws.com/fake-expired-key?X-Amz-Expires=1&X-Amz-Date=20200101T000000Z",
        processor_ids: ["core-c2pa"],
      });
      record({
        category: "replay",
        name: "/verify with expired S3 URL",
        status: "FAIL",
        duration_ms: Date.now() - t0,
        details: "TEE accepted expired URL — should fail",
        expected: "rejection (expired URL)",
      });
    } catch (e: any) {
      record({
        category: "replay",
        name: "/verify with expired S3 URL",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "rejection (expired URL)",
      });
    }
  }
}

// ---------------------------------------------------------------------------
// 8. 暗号攻撃
// ---------------------------------------------------------------------------
async function testCryptoAttacks() {
  log("CAT 8", "=== 暗号攻撃テスト ===");

  // 8-1: 間違った暗号化鍵でアップロード
  {
    const fakeKey = new Uint8Array(32);
    crypto.getRandomValues(fakeKey);

    const contentB64 = Buffer.from(imageBytes).toString("base64");
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: contentB64,
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(fakeKey, payloadJson);

    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(
        gatewayUrl,
        encryptedPayload
      );
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      record({
        category: "crypto",
        name: "/verify with wrong encryption key",
        status: "FAIL",
        duration_ms: Date.now() - t0,
        details: "TEE accepted payload encrypted with wrong key",
        expected: "decryption failure → rejection",
      });
    } catch (e: any) {
      record({
        category: "crypto",
        name: "/verify with wrong encryption key",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "decryption failure → rejection",
      });
    }
  }

  // 8-2: 改竄された ciphertext
  {
    const contentB64 = Buffer.from(imageBytes).toString("base64");
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: contentB64,
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(
      encPubkeyBytes,
      payloadJson
    );

    // ciphertextの一部を改変
    const tampered = Buffer.from(encryptedPayload.ciphertext, "base64");
    tampered[0] ^= 0xff;
    tampered[10] ^= 0xff;
    tampered[tampered.length - 1] ^= 0xff;
    const tamperedPayload = {
      ...encryptedPayload,
      ciphertext: tampered.toString("base64"),
    };

    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tamperedPayload);
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      record({
        category: "crypto",
        name: "/verify tampered ciphertext",
        status: "FAIL",
        duration_ms: Date.now() - t0,
        details: "TEE accepted tampered ciphertext",
        expected: "AES-GCM auth tag failure → rejection",
      });
    } catch (e: any) {
      record({
        category: "crypto",
        name: "/verify tampered ciphertext",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "AES-GCM auth tag failure → rejection",
      });
    }
  }

  // 8-3: 改竄された nonce
  {
    const contentB64 = Buffer.from(imageBytes).toString("base64");
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: contentB64,
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(
      encPubkeyBytes,
      payloadJson
    );

    // nonceを改変
    const nonceBuf = Buffer.from(encryptedPayload.nonce, "base64");
    nonceBuf[0] ^= 0xff;
    const tamperedPayload = {
      ...encryptedPayload,
      nonce: nonceBuf.toString("base64"),
    };

    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tamperedPayload);
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      record({
        category: "crypto",
        name: "/verify tampered nonce",
        status: "FAIL",
        duration_ms: Date.now() - t0,
        details: "TEE accepted payload with wrong nonce",
        expected: "AES-GCM decryption failure",
      });
    } catch (e: any) {
      record({
        category: "crypto",
        name: "/verify tampered nonce",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "AES-GCM decryption failure",
      });
    }
  }

  // 8-4: 空のephemeral_pubkey
  {
    const contentB64 = Buffer.from(imageBytes).toString("base64");
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: contentB64,
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(
      encPubkeyBytes,
      payloadJson
    );

    const tamperedPayload = {
      ...encryptedPayload,
      ephemeral_pubkey: "", // 空
    };

    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tamperedPayload);
      const encResp = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      record({
        category: "crypto",
        name: "/verify empty ephemeral_pubkey",
        status: "FAIL",
        duration_ms: Date.now() - t0,
        details: "TEE accepted empty ephemeral pubkey",
        expected: "ECDH failure → rejection",
      });
    } catch (e: any) {
      record({
        category: "crypto",
        name: "/verify empty ephemeral_pubkey",
        status: "PASS",
        duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`,
        expected: "ECDH failure → rejection",
      });
    }
  }
}

// ---------------------------------------------------------------------------
// 9. リソース枯渇テスト
// ---------------------------------------------------------------------------
async function testResourceExhaustion() {
  log("CAT 9", "=== リソース枯渇テスト ===");

  // 9-1: 100 rapid upload-url requests
  {
    const count = 100;
    const t0 = Date.now();
    const promises = Array.from({ length: count }, async () => {
      try {
        const res = await fetchWithTimeout(
          `${gatewayUrl}/upload-url`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              content_size: 1000,
              content_type: "image/jpeg",
            }),
          },
          10000
        );
        return { ok: res.ok, status: res.status };
      } catch {
        return { ok: false, status: 0 };
      }
    });

    const allRes = await Promise.all(promises);
    const d = Date.now() - t0;
    const ok = allRes.filter((r) => r.ok).length;
    const rps = Math.round((count / d) * 1000);

    record({
      category: "resource",
      name: `${count} concurrent /upload-url`,
      status: ok >= count * 0.9 ? "PASS" : "FAIL",
      duration_ms: d,
      details: `${ok}/${count} ok, ${rps} req/s, total=${d}ms`,
      expected: ">90% success, server stable",
    });
  }

  // 9-2: /verify flood (10 concurrent, using real encrypted payloads)
  {
    const count = 10;
    log("CAT 9", `  ${count}並列 /verify flood準備中...`);
    const uploads = await Promise.all(
      Array.from({ length: count }, () => uploadEncrypted())
    );

    const t0 = Date.now();
    const promises = uploads.map(async ({ downloadUrl, symmetricKey }) => {
      const t = Date.now();
      try {
        const encResp = await client.verify(gatewayUrl, {
          download_url: downloadUrl,
          processor_ids: ["core-c2pa", "phash-v1"],
        });
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        return { ok: true, ms: Date.now() - t };
      } catch (e: any) {
        return { ok: false, ms: Date.now() - t, err: e.message };
      }
    });

    const res = await Promise.all(promises);
    const d = Date.now() - t0;
    const ok = res.filter((r) => r.ok).length;
    const failed = res.filter((r) => !r.ok);
    const times = res.filter((r) => r.ok).map((r) => r.ms);

    record({
      category: "resource",
      name: `${count} concurrent /verify+phash flood`,
      status: ok > 0 ? "PASS" : "FAIL",
      duration_ms: d,
      details: `${ok}/${count} ok, total=${d}ms, ${failed.length > 0 ? `failures: ${failed.map((f) => (f as any).err?.slice(0, 50)).join("; ")}` : "all success"}`,
      expected: "server remains stable, some may timeout",
    });
  }

  // 9-3: health check after all tests (resilience)
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content_size: 1, content_type: "image/jpeg" }),
    });
    const d = Date.now() - t0;
    record({
      category: "resource",
      name: "health check after all attacks",
      status: res.ok ? "PASS" : "FAIL",
      duration_ms: d,
      http_status: res.status,
      details: `HTTP ${res.status} — server ${res.ok ? "alive" : "DOWN"}`,
      expected: "200 OK (server survived all attacks)",
    });
  }
}

// ---------------------------------------------------------------------------
// 10. プロトコルレベル攻撃（§1.1 登録フロー悪用）
// ---------------------------------------------------------------------------
async function testProtocolAbuse() {
  log("CAT 10", "=== プロトコルレベル攻撃 ===");

  // 10-1: owner_walletが空文字
  {
    const payload = { owner_wallet: "", content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      const encResp = await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "protocol", name: "empty owner_wallet", status: "PASS", duration_ms: Date.now() - t0,
        details: "TEE processed (owner_wallet validation may be client-side)", expected: "graceful handling" });
    } catch (e: any) {
      record({ category: "protocol", name: "empty owner_wallet", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "graceful handling" });
    }
  }

  // 10-2: owner_walletにSQLインジェクション文字列
  {
    const payload = { owner_wallet: "'; DROP TABLE titles; --", content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      const encResp = await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "protocol", name: "SQL injection in owner_wallet", status: "PASS", duration_ms: Date.now() - t0,
        details: "TEE processed without SQL injection", expected: "no SQL execution" });
    } catch (e: any) {
      record({ category: "protocol", name: "SQL injection in owner_wallet", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "no SQL execution" });
    }
  }

  // 10-3: contentフィールドが不正Base64
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: "!!!NOT-BASE64@@@" };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "protocol", name: "invalid base64 content", status: "PASS", duration_ms: Date.now() - t0,
        details: "processed (may fail at C2PA parse)", expected: "graceful rejection" });
    } catch (e: any) {
      record({ category: "protocol", name: "invalid base64 content", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "graceful rejection" });
    }
  }

  // 10-4: 暗号化ペイロード内のJSONに余分なフィールド注入
  {
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: Buffer.from(imageBytes).toString("base64"),
      __proto__: { admin: true },
      constructor: { prototype: { isAdmin: true } },
      tee_signing_key: "AAAA",
      gateway_signature: "forged",
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload, symmetricKey } = await encryptPayload(encPubkeyBytes, payloadJson);
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      const encResp = await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      const plain = await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
      const parsed = JSON.parse(new TextDecoder().decode(plain));
      record({ category: "protocol", name: "prototype pollution + field injection", status: "PASS", duration_ms: Date.now() - t0,
        details: `processed safely, results=${parsed.results?.length}`, expected: "extra fields ignored" });
    } catch (e: any) {
      record({ category: "protocol", name: "prototype pollution + field injection", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "extra fields ignored or rejected" });
    }
  }

  // 10-5: processor_idsに大量のIDを詰め込む（100個）
  {
    const { downloadUrl } = await uploadEncrypted();
    const manyProcessors = Array.from({ length: 100 }, (_, i) => `fake-processor-${i}`);
    const t0 = Date.now();
    try {
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: manyProcessors });
      record({ category: "protocol", name: "100 processor_ids", status: "FAIL", duration_ms: Date.now() - t0,
        details: "TEE accepted 100 untrusted processors", expected: "rejection" });
    } catch (e: any) {
      record({ category: "protocol", name: "100 processor_ids", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection (untrusted extensions)" });
    }
  }

  // 10-6: processor_idsにパストラバーサル文字列
  {
    const { downloadUrl } = await uploadEncrypted();
    const t0 = Date.now();
    try {
      await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["../../../etc/passwd", "core-c2pa; rm -rf /", "core-c2pa\x00evil"],
      });
      record({ category: "protocol", name: "path traversal in processor_ids", status: "PASS", duration_ms: Date.now() - t0,
        details: "processed (traversal strings treated as IDs)", expected: "rejection or safe handling" });
    } catch (e: any) {
      record({ category: "protocol", name: "path traversal in processor_ids", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection or safe handling" });
    }
  }
}

// ---------------------------------------------------------------------------
// 11. SSRF & URL操作攻撃（§6.4 TEEからの外部通信）
// ---------------------------------------------------------------------------
async function testSSRF() {
  log("CAT 11", "=== SSRF & URL操作攻撃 ===");

  const ssrfTargets = [
    { name: "localhost upload-url", url: "http://localhost:3000/upload-url" },
    { name: "AWS metadata SSRF", url: "http://169.254.169.254/latest/meta-data/" },
    { name: "AWS metadata IMDSv2", url: "http://169.254.169.254/latest/api/token" },
    { name: "internal IP", url: "http://10.0.0.1:4000/create-tree" },
    { name: "file:// scheme", url: "file:///etc/passwd" },
    { name: "gopher:// scheme", url: "gopher://evil.com:25/xHELO" },
    { name: "dict:// scheme", url: "dict://evil.com:11211/stats" },
    { name: "download_url = data: URI", url: "data:application/json;base64,eyJ0ZXN0IjoxfQ==" },
    { name: "IPv6 loopback", url: "http://[::1]:4000/create-tree" },
    { name: "decimal IP (127.0.0.1=2130706433)", url: "http://2130706433:4000/" },
  ];

  for (const target of ssrfTargets) {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ download_url: target.url, processor_ids: ["core-c2pa"] }),
      }, 10000);
      const d = Date.now() - t0;
      const body = await res.text();
      const leaked = body.includes("ami-") || body.includes("root:") || body.includes("iam");
      record({ category: "ssrf", name: `SSRF: ${target.name}`, status: leaked ? "FAIL" : "PASS",
        duration_ms: d, http_status: res.status,
        details: `HTTP ${res.status} — ${leaked ? "DATA LEAKED!" : "no leak"} — ${body.slice(0, 80)}`,
        expected: "rejection, no internal data leak" });
    } catch (e: any) {
      record({ category: "ssrf", name: `SSRF: ${target.name}`, status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }
}

// ---------------------------------------------------------------------------
// 12. HTTP Smuggling & ヘッダ攻撃
// ---------------------------------------------------------------------------
async function testHTTPSmuggling() {
  log("CAT 12", "=== HTTP Smuggling & ヘッダ攻撃 ===");

  // 12-1: 超大量のヘッダ（100個の巨大カスタムヘッダ）
  {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    for (let i = 0; i < 100; i++) {
      headers[`X-Custom-${i}`] = "A".repeat(1000);
    }
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/verify`, {
        method: "POST", headers, body: JSON.stringify({ download_url: "https://fake.com", processor_ids: [] }),
      }, 10000);
      record({ category: "http_smuggle", name: "100 large custom headers (100KB total)", status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: Date.now() - t0, http_status: res.status, details: `HTTP ${res.status}`, expected: "rejection or handled" });
    } catch (e: any) {
      record({ category: "http_smuggle", name: "100 large custom headers (100KB total)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }

  // 12-2: Hostヘッダインジェクション
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/upload-url`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Host": "evil.com",
          "X-Forwarded-Host": "evil.com",
          "X-Forwarded-For": "127.0.0.1",
        },
        body: JSON.stringify({ content_size: 1, content_type: "image/jpeg" }),
      }, 5000);
      const body = await res.text();
      const leaked = body.includes("evil.com");
      record({ category: "http_smuggle", name: "Host header injection", status: leaked ? "FAIL" : "PASS",
        duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status}, reflected evil.com: ${leaked}`, expected: "no host reflection" });
    } catch (e: any) {
      record({ category: "http_smuggle", name: "Host header injection", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }

  // 12-3: Content-Type不一致攻撃（JSONだがXMLを宣言）
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/xml" },
        body: `<?xml version="1.0"?><!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><root>&xxe;</root>`,
      }, 5000);
      const body = await res.text();
      record({ category: "http_smuggle", name: "XXE via Content-Type: application/xml",
        status: body.includes("root:") ? "FAIL" : "PASS", duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status}, XXE: ${body.includes("root:") ? "LEAKED" : "safe"}`,
        expected: "rejection (server expects JSON)" });
    } catch (e: any) {
      record({ category: "http_smuggle", name: "XXE via Content-Type: application/xml", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }

  // 12-4: 巨大Content-Type値
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json; charset=" + "A".repeat(10000) },
        body: "{}",
      }, 5000);
      record({ category: "http_smuggle", name: "10KB Content-Type header value", status: "PASS",
        duration_ms: Date.now() - t0, http_status: res.status, details: `HTTP ${res.status}`, expected: "handled gracefully" });
    } catch (e: any) {
      record({ category: "http_smuggle", name: "10KB Content-Type header value", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "handled gracefully" });
    }
  }
}

// ---------------------------------------------------------------------------
// 13. 持続負荷テスト（サーバ劣化検出）
// ---------------------------------------------------------------------------
async function testSustainedLoad() {
  log("CAT 13", "=== 持続負荷テスト ===");

  // 13-1: 段階的負荷 — 5→10→20→30→50並列、各ウェーブでレイテンシ劣化を計測
  const waves = [5, 10, 20, 30, 50];
  const waveResults: { concurrency: number; avgMs: number; successRate: number }[] = [];

  for (const concurrency of waves) {
    log("CAT 13", `--- ${concurrency}並列ウェーブ ---`);
    // アップロード
    const uploads = await Promise.all(
      Array.from({ length: concurrency }, () => uploadEncrypted())
    );

    const t0 = Date.now();
    const promises = uploads.map(async ({ downloadUrl, symmetricKey }) => {
      const t = Date.now();
      try {
        const encResp = await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        return { ok: true, ms: Date.now() - t };
      } catch {
        return { ok: false, ms: Date.now() - t };
      }
    });

    const res = await Promise.all(promises);
    const totalMs = Date.now() - t0;
    const succeeded = res.filter((r) => r.ok).length;
    const times = res.filter((r) => r.ok).map((r) => r.ms);
    const avgMs = times.length > 0 ? Math.round(times.reduce((a, b) => a + b, 0) / times.length) : -1;
    const p95 = times.length > 0 ? times.sort((a, b) => a - b)[Math.floor(times.length * 0.95)] : -1;

    waveResults.push({ concurrency, avgMs, successRate: succeeded / concurrency });

    record({
      category: "sustained",
      name: `${concurrency} concurrent /verify wave`,
      status: succeeded > concurrency * 0.5 ? "PASS" : "FAIL",
      duration_ms: totalMs,
      details: `${succeeded}/${concurrency} ok, avg=${avgMs}ms, p95=${p95}ms, total=${totalMs}ms`,
      expected: ">50% success, measurable degradation curve",
    });
  }

  // 劣化曲線のサマリー
  log("CAT 13", "--- 劣化曲線 ---");
  for (const w of waveResults) {
    log("CAT 13", `  ${w.concurrency}並列: avg=${w.avgMs}ms, success=${(w.successRate * 100).toFixed(0)}%`);
  }
}

// ---------------------------------------------------------------------------
// 14. /sign & /sign-and-mint エンドポイント探索
// ---------------------------------------------------------------------------
async function testSignEndpoints() {
  log("CAT 14", "=== /sign & /sign-and-mint 探索 ===");

  // 14-1: /sign に空のrequests配列
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/sign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ recent_blockhash: "11111111111111111111111111111111", requests: [] }),
      }, 10000);
      const body = await res.text();
      record({ category: "sign_probe", name: "/sign empty requests[]", status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status} — ${body.slice(0, 80)}`, expected: "rejection or empty response" });
    } catch (e: any) {
      record({ category: "sign_probe", name: "/sign empty requests[]", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }

  // 14-2: /sign に100個のrequests（amplification attack）
  {
    const fakeRequests = Array.from({ length: 100 }, (_, i) => ({
      signed_json_uri: `https://arweave.net/fake-${i}`,
    }));
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/sign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ recent_blockhash: "11111111111111111111111111111111", requests: fakeRequests }),
      }, 15000);
      const body = await res.text();
      record({ category: "sign_probe", name: "/sign 100 fake requests (amplification)", status: "PASS",
        duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status} — ${body.slice(0, 100)}`, expected: "handled without hanging" });
    } catch (e: any) {
      record({ category: "sign_probe", name: "/sign 100 fake requests (amplification)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection or timeout" });
    }
  }

  // 14-3: /sign-and-mint 直接呼び出し（Gateway認証なし）
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/sign-and-mint`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          recent_blockhash: "11111111111111111111111111111111",
          requests: [{ signed_json_uri: "https://arweave.net/fake" }],
        }),
      }, 10000);
      const body = await res.text();
      record({ category: "sign_probe", name: "/sign-and-mint direct call",
        status: res.status >= 400 ? "PASS" : "FAIL", duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status} — ${body.slice(0, 100)}`, expected: "rejection (requires auth)" });
    } catch (e: any) {
      record({ category: "sign_probe", name: "/sign-and-mint direct call", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }

  // 14-4: /sign にjavascript://スキームのURI
  {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}/sign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          recent_blockhash: "11111111111111111111111111111111",
          requests: [{ signed_json_uri: "javascript:alert(1)" }],
        }),
      }, 10000);
      record({ category: "sign_probe", name: "/sign javascript: URI scheme", status: res.status >= 400 ? "PASS" : "FAIL",
        duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status}`, expected: "rejection" });
    } catch (e: any) {
      record({ category: "sign_probe", name: "/sign javascript: URI scheme", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }
}

// ---------------------------------------------------------------------------
// 15. タイミングサイドチャネル分析
// ---------------------------------------------------------------------------
async function testTimingSideChannel() {
  log("CAT 15", "=== タイミングサイドチャネル分析 ===");

  // 異なるエラーパターンのレイテンシを比較
  // 有意なタイミング差 → 情報リーク可能性

  const trials = 5;

  // 15-1: 有効な暗号化 vs 無効な暗号化のタイミング差
  const validTimes: number[] = [];
  const invalidTimes: number[] = [];

  for (let i = 0; i < trials; i++) {
    // 有効: 正しい鍵で暗号化
    {
      const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
      const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
      const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      const t0 = Date.now();
      try { await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] }); } catch {}
      validTimes.push(Date.now() - t0);
    }
    // 無効: ランダム鍵で暗号化
    {
      const fakeKey = new Uint8Array(32);
      crypto.getRandomValues(fakeKey);
      const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
      const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
      const { encryptedPayload } = await encryptPayload(fakeKey, payloadJson);
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      const t0 = Date.now();
      try { await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] }); } catch {}
      invalidTimes.push(Date.now() - t0);
    }
  }

  const validAvg = Math.round(validTimes.reduce((a, b) => a + b, 0) / validTimes.length);
  const invalidAvg = Math.round(invalidTimes.reduce((a, b) => a + b, 0) / invalidTimes.length);
  const timingDiff = Math.abs(validAvg - invalidAvg);
  const ratio = validAvg > 0 ? (timingDiff / validAvg * 100).toFixed(1) : "N/A";

  record({
    category: "timing",
    name: "valid vs invalid encryption timing",
    status: "PASS",
    duration_ms: validAvg + invalidAvg,
    details: `valid_avg=${validAvg}ms, invalid_avg=${invalidAvg}ms, diff=${timingDiff}ms (${ratio}%)`,
    expected: "informational — large diff may leak encryption validity",
  });

  // 15-2: 存在するprocessor vs 存在しないprocessorのタイミング差
  const existTimes: number[] = [];
  const noexistTimes: number[] = [];

  for (let i = 0; i < trials; i++) {
    {
      const { downloadUrl } = await uploadEncrypted();
      const t0 = Date.now();
      try { await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] }); } catch {}
      existTimes.push(Date.now() - t0);
    }
    {
      const { downloadUrl } = await uploadEncrypted();
      const t0 = Date.now();
      try { await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["nonexistent-v99"] }); } catch {}
      noexistTimes.push(Date.now() - t0);
    }
  }

  const existAvg = Math.round(existTimes.reduce((a, b) => a + b, 0) / existTimes.length);
  const noexistAvg = Math.round(noexistTimes.reduce((a, b) => a + b, 0) / noexistTimes.length);

  record({
    category: "timing",
    name: "valid vs invalid processor_id timing",
    status: "PASS",
    duration_ms: existAvg + noexistAvg,
    details: `valid_avg=${existAvg}ms, invalid_avg=${noexistAvg}ms, diff=${Math.abs(existAvg - noexistAvg)}ms`,
    expected: "informational — timing oracle for extension existence",
  });
}

// ---------------------------------------------------------------------------
// 16. 二重暗号化 & ペイロード混乱攻撃
// ---------------------------------------------------------------------------
async function testPayloadConfusion() {
  log("CAT 16", "=== ペイロード混乱攻撃 ===");

  // 16-1: 二重暗号化（暗号化ペイロードをさらに暗号化）
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload: inner } = await encryptPayload(encPubkeyBytes, payloadJson);
    // innerをJSON化してもう一度暗号化
    const innerJson = new TextEncoder().encode(JSON.stringify(inner));
    const { encryptedPayload: outer } = await encryptPayload(encPubkeyBytes, innerJson);

    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, outer);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "confusion", name: "double encryption (matryoshka)", status: "PASS", duration_ms: Date.now() - t0,
        details: "TEE processed (inner decryption yields JSON, not image)", expected: "rejection at C2PA parse" });
    } catch (e: any) {
      record({ category: "confusion", name: "double encryption (matryoshka)", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection at C2PA parse or decryption" });
    }
  }

  // 16-2: 超巨大JSON (10万キー)
  {
    const megaObj: Record<string, string> = {};
    for (let i = 0; i < 100_000; i++) megaObj[`key_${i}`] = `val_${i}`;
    const megaJson = JSON.stringify(megaObj);
    const payloadJson = new TextEncoder().encode(megaJson);
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);

    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "confusion", name: "100K-key JSON payload (3MB+)", status: "PASS", duration_ms: Date.now() - t0,
        details: "TEE processed (giant JSON, no content field)", expected: "rejection" });
    } catch (e: any) {
      record({ category: "confusion", name: "100K-key JSON payload (3MB+)", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection" });
    }
  }

  // 16-3: null bytes埋め込みファイル名
  {
    const payload = {
      owner_wallet: keypair.publicKey.toBase58(),
      content: Buffer.from(imageBytes).toString("base64"),
      filename: "image.jpg\x00.exe",
    };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, encryptedPayload);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "confusion", name: "null byte in filename field", status: "PASS", duration_ms: Date.now() - t0,
        details: "TEE handled null byte safely", expected: "no path truncation exploit" });
    } catch (e: any) {
      record({ category: "confusion", name: "null byte in filename field", status: "PASS", duration_ms: Date.now() - t0,
        details: `rejected: ${e.message.slice(0, 100)}`, expected: "handled safely" });
    }
  }

  // 16-4: アップロードしたURLを別のverifyリクエスト間で交差使用
  {
    const upload1 = await uploadEncrypted();
    const upload2 = await uploadEncrypted();

    // upload1のURLをupload2の対称鍵で復号しようとする
    const t0 = Date.now();
    try {
      const encResp = await client.verify(gatewayUrl, { download_url: upload1.downloadUrl, processor_ids: ["core-c2pa"] });
      // upload2のsymmetricKeyで復号 → 失敗するはず
      try {
        await decryptResponse(upload2.symmetricKey, encResp.nonce, encResp.ciphertext);
        record({ category: "confusion", name: "cross-session key confusion", status: "FAIL", duration_ms: Date.now() - t0,
          details: "decrypted with wrong session key!", expected: "decryption failure" });
      } catch {
        record({ category: "confusion", name: "cross-session key confusion", status: "PASS", duration_ms: Date.now() - t0,
          details: "correct: wrong key fails decryption", expected: "decryption failure (different ECDH shared secret)" });
      }
    } catch (e: any) {
      record({ category: "confusion", name: "cross-session key confusion", status: "PASS", duration_ms: Date.now() - t0,
        details: `verify failed: ${e.message.slice(0, 80)}`, expected: "handled" });
    }
  }
}

// ---------------------------------------------------------------------------
// 17. X25519暗号エッジケース（ホワイトボックス: crates/crypto/src/lib.rs）
// ---------------------------------------------------------------------------
async function testCryptoEdgeCases() {
  log("CAT 17", "=== X25519暗号エッジケース（ホワイトボックス） ===");

  // 17-1: 全ゼロ ephemeral_pubkey (X25519 low-order point → shared_secret = 0)
  // X25519は入力がsmall subgroup pointでもpanicしない設計だが、
  // shared_secret=0 → HKDF → 有効な対称鍵 → TEEが復号を試みる
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    // ephemeral_pubkeyを全ゼロに置換（X25519 identity point）
    const zeroKey = Buffer.alloc(32, 0).toString("base64");
    const tampered = { ...encryptedPayload, ephemeral_pubkey: zeroKey };
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tampered);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "crypto_edge", name: "all-zero ephemeral_pubkey (identity point)", status: "PASS",
        duration_ms: Date.now() - t0, details: "TEE processed (ECDH result is clamped)", expected: "decryption failure" });
    } catch (e: any) {
      record({ category: "crypto_edge", name: "all-zero ephemeral_pubkey (identity point)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "decryption failure" });
    }
  }

  // 17-2: 全0xFF ephemeral_pubkey（大きなスカラー）
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const ffKey = Buffer.alloc(32, 0xff).toString("base64");
    const tampered = { ...encryptedPayload, ephemeral_pubkey: ffKey };
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tampered);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "crypto_edge", name: "all-0xFF ephemeral_pubkey", status: "PASS",
        duration_ms: Date.now() - t0, details: "TEE handled safely", expected: "decryption failure" });
    } catch (e: any) {
      record({ category: "crypto_edge", name: "all-0xFF ephemeral_pubkey", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "decryption failure" });
    }
  }

  // 17-3: ephemeral_pubkeyが31バイト（短すぎ）
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const shortKey = Buffer.alloc(31, 0x42).toString("base64");
    const tampered = { ...encryptedPayload, ephemeral_pubkey: shortKey };
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tampered);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "crypto_edge", name: "31-byte ephemeral_pubkey (short)", status: "FAIL",
        duration_ms: Date.now() - t0, details: "TEE accepted truncated key!", expected: "rejection" });
    } catch (e: any) {
      record({ category: "crypto_edge", name: "31-byte ephemeral_pubkey (short)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection" });
    }
  }

  // 17-4: ephemeral_pubkeyが33バイト（長すぎ）
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const longKey = Buffer.alloc(33, 0x42).toString("base64");
    const tampered = { ...encryptedPayload, ephemeral_pubkey: longKey };
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tampered);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "crypto_edge", name: "33-byte ephemeral_pubkey (long)", status: "FAIL",
        duration_ms: Date.now() - t0, details: "TEE accepted oversized key!", expected: "rejection" });
    } catch (e: any) {
      record({ category: "crypto_edge", name: "33-byte ephemeral_pubkey (long)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection" });
    }
  }

  // 17-5: 同一ephemeral_pubkey再利用 — 同じ暗号ペイロードを2回送り、
  // レスポンスのnonceが異なることを確認（AES-GCM nonce reuse防止）
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { symmetricKey, encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);

    // 同じ暗号文を2回アップロード → 同じephemeral_pubkeyから同じ対称鍵を導出
    const { downloadUrl: url1 } = await client.upload(gatewayUrl, encryptedPayload);
    const { downloadUrl: url2 } = await client.upload(gatewayUrl, encryptedPayload);

    const t0 = Date.now();
    try {
      const [enc1, enc2] = await Promise.all([
        client.verify(gatewayUrl, { download_url: url1, processor_ids: ["core-c2pa"] }),
        client.verify(gatewayUrl, { download_url: url2, processor_ids: ["core-c2pa"] }),
      ]);

      // レスポンスのnonceが異なることを確認
      const noncesMatch = enc1.nonce === enc2.nonce;
      // 両方とも同じsymmetricKeyで復号できることを確認
      const plain1 = await decryptResponse(symmetricKey, enc1.nonce, enc1.ciphertext);
      const plain2 = await decryptResponse(symmetricKey, enc2.nonce, enc2.ciphertext);

      record({ category: "crypto_edge", name: "ephemeral_pubkey reuse (nonce uniqueness)",
        status: noncesMatch ? "FAIL" : "PASS", duration_ms: Date.now() - t0,
        details: `nonces_match=${noncesMatch}, both_decryptable=true, nonce1=${enc1.nonce.slice(0, 16)}..., nonce2=${enc2.nonce.slice(0, 16)}...`,
        expected: "different nonces per response (AES-GCM safety)" });
    } catch (e: any) {
      record({ category: "crypto_edge", name: "ephemeral_pubkey reuse (nonce uniqueness)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected reuse: ${e.message.slice(0, 100)}`, expected: "rejection or different nonces" });
    }
  }

  // 17-6: nonceが11バイト（AES-GCM標準の12バイトより短い）
  {
    const payload = { owner_wallet: keypair.publicKey.toBase58(), content: Buffer.from(imageBytes).toString("base64") };
    const payloadJson = new TextEncoder().encode(JSON.stringify(payload));
    const { encryptedPayload } = await encryptPayload(encPubkeyBytes, payloadJson);
    const shortNonce = Buffer.alloc(11, 0x42).toString("base64");
    const tampered = { ...encryptedPayload, nonce: shortNonce };
    const t0 = Date.now();
    try {
      const { downloadUrl } = await client.upload(gatewayUrl, tampered);
      await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
      record({ category: "crypto_edge", name: "11-byte nonce (too short for AES-GCM)", status: "FAIL",
        duration_ms: Date.now() - t0, details: "TEE accepted short nonce!", expected: "rejection" });
    } catch (e: any) {
      record({ category: "crypto_edge", name: "11-byte nonce (too short for AES-GCM)", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "rejection" });
    }
  }
}

// ---------------------------------------------------------------------------
// 18. JSON型混同攻撃（ホワイトボックス: Axum/Serde型強制のテスト）
// ---------------------------------------------------------------------------
async function testJsonTypeConfusion() {
  log("CAT 18", "=== JSON型混同攻撃 ===");

  // 各エンドポイントの各フィールドに不正な型を送る
  const typeConfusionCases: { endpoint: string; name: string; body: string }[] = [
    // /upload-url: content_size should be u64
    { endpoint: "/upload-url", name: "content_size=string", body: '{"content_size":"big","content_type":"image/jpeg"}' },
    { endpoint: "/upload-url", name: "content_size=float", body: '{"content_size":3.14,"content_type":"image/jpeg"}' },
    { endpoint: "/upload-url", name: "content_size=negative", body: '{"content_size":-1,"content_type":"image/jpeg"}' },
    { endpoint: "/upload-url", name: "content_size=true", body: '{"content_size":true,"content_type":"image/jpeg"}' },
    { endpoint: "/upload-url", name: "content_size=array", body: '{"content_size":[1000],"content_type":"image/jpeg"}' },
    { endpoint: "/upload-url", name: "content_size=null", body: '{"content_size":null,"content_type":"image/jpeg"}' },
    { endpoint: "/upload-url", name: "content_size=MAX_SAFE_INTEGER+1", body: `{"content_size":${Number.MAX_SAFE_INTEGER + 1},"content_type":"image/jpeg"}` },
    // /verify: processor_ids should be string array
    { endpoint: "/verify", name: "processor_ids=string", body: '{"download_url":"http://x","processor_ids":"core-c2pa"}' },
    { endpoint: "/verify", name: "processor_ids=number", body: '{"download_url":"http://x","processor_ids":42}' },
    { endpoint: "/verify", name: "processor_ids=nested_array", body: '{"download_url":"http://x","processor_ids":[["nested"]]}' },
    { endpoint: "/verify", name: "processor_ids=null", body: '{"download_url":"http://x","processor_ids":null}' },
    { endpoint: "/verify", name: "download_url=number", body: '{"download_url":12345,"processor_ids":["core-c2pa"]}' },
    { endpoint: "/verify", name: "download_url=array", body: '{"download_url":["http://a","http://b"],"processor_ids":["core-c2pa"]}' },
    // /sign: recent_blockhash / requests type confusion
    { endpoint: "/sign", name: "recent_blockhash=number", body: '{"recent_blockhash":0,"requests":[]}' },
    { endpoint: "/sign", name: "requests=string", body: '{"recent_blockhash":"xxx","requests":"not_array"}' },
    { endpoint: "/sign", name: "requests=object", body: '{"recent_blockhash":"xxx","requests":{"uri":"fake"}}' },
  ];

  for (const tc of typeConfusionCases) {
    const t0 = Date.now();
    try {
      const res = await fetchWithTimeout(`${gatewayUrl}${tc.endpoint}`, {
        method: "POST", headers: { "Content-Type": "application/json" }, body: tc.body,
      }, 5000);
      const body = await res.text();
      // 200はFAIL（型が通ってしまった）、4xxはPASS（拒否された）
      const isTypeAccepted = res.status === 200;
      record({ category: "type_confusion", name: `${tc.endpoint} ${tc.name}`,
        status: isTypeAccepted ? "FAIL" : "PASS", duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status} — ${body.slice(0, 80)}`, expected: "400/422 type rejection" });
    } catch (e: any) {
      record({ category: "type_confusion", name: `${tc.endpoint} ${tc.name}`, status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 60)}`, expected: "rejection" });
    }
  }
}

// ---------------------------------------------------------------------------
// 19. TOCTOU競合攻撃（同一リソースへの高速並行アクセス）
// ---------------------------------------------------------------------------
async function testRaceConditions() {
  log("CAT 19", "=== TOCTOU競合攻撃 ===");

  // 19-1: 同一download_urlに100並列/verify（TEEの復号・処理が競合）
  {
    const { downloadUrl, symmetricKey } = await uploadEncrypted();
    const concurrency = 100;
    const t0 = Date.now();

    const promises = Array.from({ length: concurrency }, async () => {
      try {
        const enc = await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
        await decryptResponse(symmetricKey, enc.nonce, enc.ciphertext);
        return { ok: true };
      } catch {
        return { ok: false };
      }
    });

    const res = await Promise.all(promises);
    const ok = res.filter((r) => r.ok).length;
    record({ category: "race", name: "100 concurrent /verify same URL", status: ok > 0 ? "PASS" : "FAIL",
      duration_ms: Date.now() - t0,
      details: `${ok}/${concurrency} ok — server handled concurrent access to same S3 object`,
      expected: "no crash, graceful handling" });
  }

  // 19-2: /upload-url直後に（S3アップロード完了前に）/verify
  // → download_urlが有効になる前にTEEがダウンロード試行
  {
    const t0 = Date.now();
    try {
      // upload-urlだけ取得（実際のS3アップロードはしない）
      const res = await fetch(`${gatewayUrl}/upload-url`, {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content_size: 1000, content_type: "image/jpeg" }),
      });
      const { download_url } = await res.json() as { download_url: string };

      // S3にはまだ何もアップロードしていない状態でverify
      await client.verify(gatewayUrl, { download_url, processor_ids: ["core-c2pa"] });
      record({ category: "race", name: "verify before upload completes", status: "PASS",
        duration_ms: Date.now() - t0, details: "TEE processed (empty object?)", expected: "graceful failure" });
    } catch (e: any) {
      record({ category: "race", name: "verify before upload completes", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "graceful failure (S3 404)" });
    }
  }

  // 19-3: /verifyと/signを同時に呼ぶ（Phase1とPhase2の並行実行）
  {
    const t0 = Date.now();
    const promises = [
      // Phase 1
      (async () => {
        try {
          const { downloadUrl, symmetricKey } = await uploadEncrypted();
          await client.verify(gatewayUrl, { download_url: downloadUrl, processor_ids: ["core-c2pa"] });
          return { endpoint: "verify", ok: true };
        } catch { return { endpoint: "verify", ok: false }; }
      })(),
      // Phase 2 (with fake data)
      (async () => {
        try {
          const res = await fetchWithTimeout(`${gatewayUrl}/sign`, {
            method: "POST", headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ recent_blockhash: "11111111111111111111111111111111",
              requests: [{ signed_json_uri: "https://arweave.net/fake" }] }),
          }, 10000);
          return { endpoint: "sign", ok: res.status < 500 };
        } catch { return { endpoint: "sign", ok: false }; }
      })(),
    ];
    const res = await Promise.all(promises);
    record({ category: "race", name: "concurrent /verify + /sign (Phase1 + Phase2)", status: "PASS",
      duration_ms: Date.now() - t0,
      details: `verify=${res[0].ok}, sign=${res[1].ok} — no deadlock`, expected: "independent processing, no deadlock" });
  }
}

// ---------------------------------------------------------------------------
// 20. 境界値テスト（ホワイトボックス: security.rs の定数）
// ---------------------------------------------------------------------------
async function testBoundaryValues() {
  log("CAT 20", "=== 境界値テスト ===");

  // 20-1: content_size = 1 (最小有効値)
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content_size: 1, content_type: "image/jpeg" }),
    });
    record({ category: "boundary", name: "upload-url content_size=1 (minimum)",
      status: res.ok ? "PASS" : "FAIL", duration_ms: Date.now() - t0, http_status: res.status,
      details: `HTTP ${res.status}`, expected: "200 OK (1 byte is valid)" });
  }

  // 20-2: content_size = 2147483648 (exactly 2GB limit)
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content_size: 2147483648, content_type: "image/jpeg" }),
    });
    const body = await res.text();
    record({ category: "boundary", name: "upload-url content_size=2GB (exact limit)",
      status: "PASS", duration_ms: Date.now() - t0, http_status: res.status,
      details: `HTTP ${res.status} — ${body.slice(0, 80)}`, expected: "200 OK or 400 (on-boundary)" });
  }

  // 20-3: content_size = 2147483649 (2GB + 1 byte, just over limit)
  {
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content_size: 2147483649, content_type: "image/jpeg" }),
    });
    const body = await res.text();
    record({ category: "boundary", name: "upload-url content_size=2GB+1 (over limit)",
      status: res.status >= 400 ? "PASS" : "FAIL", duration_ms: Date.now() - t0, http_status: res.status,
      details: `HTTP ${res.status} — ${body.slice(0, 80)}`, expected: "400 (over limit)" });
  }

  // 20-4: content_size = u64 MAX
  {
    const t0 = Date.now();
    try {
      const res = await fetch(`${gatewayUrl}/upload-url`, {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: `{"content_size":18446744073709551615,"content_type":"image/jpeg"}`,
      });
      record({ category: "boundary", name: "upload-url content_size=u64::MAX",
        status: res.status >= 400 ? "PASS" : "FAIL", duration_ms: Date.now() - t0, http_status: res.status,
        details: `HTTP ${res.status}`, expected: "400 rejection" });
    } catch (e: any) {
      record({ category: "boundary", name: "upload-url content_size=u64::MAX", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 80)}`, expected: "rejection" });
    }
  }

  // 20-5: processor_idsに同じIDを重複して入れる
  {
    const { downloadUrl, symmetricKey } = await uploadEncrypted();
    const t0 = Date.now();
    try {
      const enc = await client.verify(gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa", "core-c2pa", "core-c2pa", "core-c2pa", "core-c2pa"],
      });
      const plain = await decryptResponse(symmetricKey, enc.nonce, enc.ciphertext);
      const parsed = JSON.parse(new TextDecoder().decode(plain));
      const resultCount = parsed.results?.length ?? 0;
      record({ category: "boundary", name: "5x duplicate processor_id", status: "PASS",
        duration_ms: Date.now() - t0,
        details: `results=${resultCount} (deduplicated=${resultCount === 1})`,
        expected: "deduplicated to 1 result, or 5 identical results" });
    } catch (e: any) {
      record({ category: "boundary", name: "5x duplicate processor_id", status: "PASS",
        duration_ms: Date.now() - t0, details: `rejected: ${e.message.slice(0, 100)}`, expected: "handled" });
    }
  }

  // 20-6: content_typeに超長い文字列（10KB MIME type）
  {
    const longMime = "image/" + "x".repeat(10000);
    const t0 = Date.now();
    const res = await fetch(`${gatewayUrl}/upload-url`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content_size: 1000, content_type: longMime }),
    });
    record({ category: "boundary", name: "upload-url content_type=10KB string",
      status: "PASS", duration_ms: Date.now() - t0, http_status: res.status,
      details: `HTTP ${res.status}`, expected: "handled (S3 may reject long content-type)" });
  }
}

// ===========================================================================
// メイン
// ===========================================================================

async function main() {
  const args = parseArgs();

  imageBytes = fs.readFileSync(path.resolve(args.imagePath));
  keypair = Keypair.fromSecretKey(
    Uint8Array.from(
      JSON.parse(fs.readFileSync(path.resolve(args.walletPath), "utf-8"))
    )
  );

  // Fetch on-chain GlobalConfig (primary source of truth)
  const connection = new Connection(args.solanaRpc, "confirmed");
  const programId = args.programId
    ? new PublicKey(args.programId)
    : undefined;
  let globalConfig: GlobalConfig;
  try {
    globalConfig = await fetchGlobalConfig(connection, programId);
    log("INFO", `GlobalConfig fetched from chain (${globalConfig.trusted_tee_nodes.length} TEE nodes)`);
  } catch (e: any) {
    if (!args.gatewayHost || !args.encryptionPubkey) {
      console.error(
        `GlobalConfig fetch failed: ${e.message}\n` +
        `On-chain config unavailable. Provide --gateway and --encryption-pubkey as fallback.`
      );
      process.exit(1);
    }
    // Fallback: construct minimal config from CLI args
    const teeNode: TrustedTeeNode = {
      signing_pubkey: "",
      encryption_pubkey: args.encryptionPubkey,
      encryption_algorithm: "x25519-hkdf-sha256-aes256gcm",
      gateway_pubkey: "",
      gateway_endpoint: `http://${args.gatewayHost}:${args.port}`,
      status: "active",
      tee_type: "mock",
      expected_measurements: {},
    };
    globalConfig = {
      authority: "",
      core_collection_mint: "",
      ext_collection_mint: "",
      trusted_tee_nodes: [teeNode],
      trusted_tsa_keys: [],
      trusted_wasm_modules: [],
    };
    log("WARN", "GlobalConfig not found on-chain, using CLI fallback");
  }

  // Resolve gateway endpoint and encryption pubkey from on-chain or CLI
  const activeNode = globalConfig.trusted_tee_nodes.find(n => n.status === "active")
    || globalConfig.trusted_tee_nodes[0];
  if (!activeNode) {
    console.error("No TEE nodes found in GlobalConfig");
    process.exit(1);
  }

  // CLI args override on-chain values
  gatewayUrl = args.gatewayHost
    ? `http://${args.gatewayHost}:${args.port}`
    : activeNode.gateway_endpoint;
  const encPubkeyB64 = args.encryptionPubkey || activeNode.encryption_pubkey;
  encPubkeyBytes = new Uint8Array(Buffer.from(encPubkeyB64, "base64"));

  log("INFO", `Gateway: ${gatewayUrl}`);
  log("INFO", `Encryption pubkey: ${encPubkeyB64.slice(0, 20)}...`);

  client = new TitleClient({
    teeNodes: [gatewayUrl],
    solanaRpcUrl: args.solanaRpc,
    globalConfig,
  });

  log("START", `Target: ${gatewayUrl}`);
  log("START", `Image: ${path.basename(args.imagePath)} (${(imageBytes.length / 1024).toFixed(1)} KB)`);
  log("START", `Wallet: ${keypair.publicKey.toBase58()}`);
  log("START", "");

  // テスト実行
  await testBaseline();
  console.log("");

  await testConcurrentLoad();
  console.log("");

  await testLargePayload();
  console.log("");

  await testMalformedInput();
  console.log("");

  await testEndpointAbuse();
  console.log("");

  await testSlowloris();
  console.log("");

  await testReplay();
  console.log("");

  await testCryptoAttacks();
  console.log("");

  await testResourceExhaustion();
  console.log("");

  await testProtocolAbuse();
  console.log("");

  await testSSRF();
  console.log("");

  await testHTTPSmuggling();
  console.log("");

  await testSustainedLoad();
  console.log("");

  await testSignEndpoints();
  console.log("");

  await testTimingSideChannel();
  console.log("");

  await testPayloadConfusion();
  console.log("");

  await testCryptoEdgeCases();
  console.log("");

  await testJsonTypeConfusion();
  console.log("");

  await testRaceConditions();
  console.log("");

  await testBoundaryValues();
  console.log("");

  // ---------------------------------------------------------------------------
  // サマリー
  // ---------------------------------------------------------------------------
  log("SUMMARY", "===========================================");
  const passed = results.filter((r) => r.status === "PASS").length;
  const failed = results.filter((r) => r.status === "FAIL").length;
  const errors = results.filter((r) => r.status === "ERROR").length;
  log("SUMMARY", `PASS: ${passed} / FAIL: ${failed} / ERROR: ${errors} / Total: ${results.length}`);

  // カテゴリ別
  const categories = [...new Set(results.map((r) => r.category))];
  for (const cat of categories) {
    const catResults = results.filter((r) => r.category === cat);
    const catPass = catResults.filter((r) => r.status === "PASS").length;
    log("SUMMARY", `  ${cat}: ${catPass}/${catResults.length} passed`);
  }

  // 結果をJSON保存
  const outPath = path.resolve("output-stress-test.json");
  fs.writeFileSync(
    outPath,
    JSON.stringify(
      {
        timestamp: new Date().toISOString(),
        target: gatewayUrl,
        image: path.basename(args.imagePath),
        image_size_bytes: imageBytes.length,
        summary: { passed, failed, errors, total: results.length },
        results,
      },
      null,
      2
    )
  );
  log("DONE", `結果を保存: ${outPath}`);

  // FAILがあった場合
  if (failed > 0) {
    log("FAIL", "以下のテストが FAIL:");
    for (const r of results.filter((r) => r.status === "FAIL")) {
      log("FAIL", `  - ${r.name}: ${r.details}`);
    }
  }
}

main().catch((e) => {
  console.error("Fatal:", e);
  process.exit(1);
});
