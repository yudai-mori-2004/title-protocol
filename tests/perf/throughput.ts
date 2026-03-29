#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Throughput saturation test.
 *
 * Sends increasing parallel requests to find the TEE node's saturation point.
 * At each concurrency level, measures latency percentiles and success rate.
 * The saturation point is where p95 latency sharply increases or success rate drops.
 *
 * Usage: npx tsx tests/perf/throughput.ts
 */

import {
  init, uploadEncrypted, loadFixture, calcStats, formatStats, getClient, getSession,
} from "./helpers.ts";
import { decryptResponse } from "@title-protocol/sdk";

const CONCURRENCY_LEVELS = [1, 2, 4, 8, 16, 32, 48, 64];
const CONTENT_PATH = "tests/fixtures/c2pa/signed/jpeg-640x480.jpg";

interface WaveResult {
  concurrency: number;
  successCount: number;
  totalMs: number;
  durations: number[];
  errors: string[];
}

async function runWave(content: Uint8Array, concurrency: number): Promise<WaveResult> {
  const client = getClient();
  const session = getSession();

  // Pre-upload all payloads
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
        errors.push(e.message?.slice(0, 80) ?? "unknown");
        return { ok: false, ms: Date.now() - t };
      }
    }),
  );

  return {
    concurrency,
    successCount: results.filter(r => r.ok).length,
    totalMs: Date.now() - t0,
    durations: results.filter(r => r.ok).map(r => r.ms),
    errors,
  };
}

async function main() {
  await init();

  const content = loadFixture(CONTENT_PATH);
  console.log("=== Throughput Saturation Test ===\n");
  console.log("Content: JPEG 640x480 (30KB), Processor: core-c2pa\n");
  console.log("Concurrency | Success | Total(ms) | Avg(ms) | p50   | p95   | p99   | RPS");
  console.log("-".repeat(90));

  for (const n of CONCURRENCY_LEVELS) {
    const wave = await runWave(content, n);
    const stats = wave.durations.length > 0 ? calcStats(wave.durations) : null;
    const rps = wave.totalMs > 0 ? ((wave.successCount / wave.totalMs) * 1000).toFixed(1) : "0";

    console.log(
      `${String(n).padStart(11)} | ${String(wave.successCount).padStart(4)}/${n} ` +
      `| ${String(wave.totalMs).padStart(9)} ` +
      `| ${String(stats?.avg ?? "-").padStart(7)} ` +
      `| ${String(stats?.p50 ?? "-").padStart(5)} ` +
      `| ${String(stats?.p95 ?? "-").padStart(5)} ` +
      `| ${String(stats?.p99 ?? "-").padStart(5)} ` +
      `| ${rps.padStart(5)}`
    );

    if (wave.errors.length > 0) {
      console.log(`            Errors: ${wave.errors.slice(0, 3).join("; ")}`);
    }

    // If success rate drops below 50%, TEE is saturated — stop
    if (wave.successCount < n * 0.5) {
      console.log(`\n*** Saturation detected at concurrency=${n} ***`);
      break;
    }
  }
}

main().catch(console.error);
