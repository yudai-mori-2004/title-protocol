#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Large file × parallel requests test.
 *
 * Tests memory pressure: large content held simultaneously in TEE memory.
 * This is where Enclave memory allocation matters most.
 *
 * Usage: npx tsx tests/perf/large-parallel.ts
 */

import {
  init, uploadEncrypted, loadFixture, calcStats, getClient, getSession,
} from "./helpers.ts";
import { decryptResponse } from "@title-protocol/sdk";

const CONCURRENCIES = [1, 2, 4, 8, 16];

const SIZES = [
  { size: "10MB", path: "tests/fixtures/c2pa/signed/mp4-10mb.mp4" },
  { size: "25MB", path: "tests/fixtures/c2pa/signed/mp4-25mb.mp4" },
  { size: "50MB", path: "tests/fixtures/c2pa/signed/mp4-50mb.mp4" },
  { size: "100MB", path: "tests/fixtures/c2pa/signed/mp4-100mb.mp4" },
  { size: "200MB", path: "tests/fixtures/c2pa/signed/mp4-200mb.mp4" },
  { size: "500MB", path: "tests/fixtures/c2pa/signed/mp4-500mb.mp4" },
];

async function runWave(content: Uint8Array, concurrency: number): Promise<{
  successCount: number;
  totalMs: number;
  durations: number[];
  errors: string[];
}> {
  const client = getClient();
  const session = getSession();

  // Pre-upload all
  const uploads = await Promise.all(
    Array.from({ length: concurrency }, () => uploadEncrypted(content)),
  );

  const errors: string[] = [];
  const t0 = Date.now();

  const results = await Promise.all(
    uploads.map(async ({ downloadUrl, symmetricKey }) => {
      const t = Date.now();
      try {
        const encResp = await client.verifyRaw(session.gatewayUrl, {
          download_url: downloadUrl,
          processor_ids: ["core-c2pa"],
        });
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        return { ok: true, ms: Date.now() - t };
      } catch (e: any) {
        errors.push(e.message?.slice(0, 60) ?? "unknown");
        return { ok: false, ms: Date.now() - t };
      }
    }),
  );

  return {
    successCount: results.filter(r => r.ok).length,
    totalMs: Date.now() - t0,
    durations: results.filter(r => r.ok).map(r => r.ms),
    errors,
  };
}

async function main() {
  await init();

  console.log("=== Large File × Parallel Requests ===\n");
  console.log("Content    | Concurrency | Success | Total(ms) | Avg(ms) | p95(ms) | Status");
  console.log("-".repeat(85));

  for (const entry of SIZES) {
    let content: Uint8Array;
    try {
      content = loadFixture(entry.path);
    } catch {
      for (const n of CONCURRENCIES) {
        console.log(`${entry.size.padEnd(11)}| ${String(n).padStart(11)} | ${"—".padStart(7)} | ${"—".padStart(9)} | ${"—".padStart(7)} | ${"—".padStart(7)} | file not found`);
      }
      continue;
    }

    for (const n of CONCURRENCIES) {
      const wave = await runWave(content, n);
      const stats = wave.durations.length > 0 ? calcStats(wave.durations) : null;
      const status = wave.successCount === n
        ? "OK"
        : wave.errors.length > 0
          ? `FAIL: ${wave.errors[0].slice(0, 30)}`
          : "FAIL";

      console.log(
        `${entry.size.padEnd(11)}| ${String(n).padStart(11)} ` +
        `| ${String(wave.successCount).padStart(4)}/${n} ` +
        `| ${String(wave.totalMs).padStart(9)} ` +
        `| ${String(stats?.avg ?? "—").padStart(7)} ` +
        `| ${String(stats?.p95 ?? "—").padStart(7)} ` +
        `| ${status}`
      );
    }
  }
}

main().catch(console.error);
