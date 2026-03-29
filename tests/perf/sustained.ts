#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Sustained load test.
 *
 * Sends requests at a constant rate for a fixed duration.
 * Detects memory leaks, ResourcePool exhaustion, and latency drift.
 *
 * Usage: npx tsx tests/perf/sustained.ts [--duration 120] [--rps 2]
 */

import {
  init, uploadEncrypted, loadFixture, calcStats, getClient, getSession,
} from "./helpers.ts";
import { decryptResponse } from "@title-protocol/sdk";

const CONTENT_PATH = "tests/fixtures/c2pa/signed/jpeg-640x480.jpg";

function parseArgs() {
  const args = process.argv.slice(2);
  let durationSecs = 120; // 2 minutes
  let targetRps = 2;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--duration") durationSecs = parseInt(args[++i], 10);
    if (args[i] === "--rps") targetRps = parseFloat(args[++i]);
  }
  return { durationSecs, targetRps };
}

async function main() {
  await init();
  const { durationSecs, targetRps } = parseArgs();

  const content = loadFixture(CONTENT_PATH);
  const client = getClient();
  const session = getSession();

  console.log("=== Sustained Load Test ===\n");
  console.log(`Duration: ${durationSecs}s, Target RPS: ${targetRps}\n`);

  const intervalMs = 1000 / targetRps;
  const startTime = Date.now();
  const endTime = startTime + durationSecs * 1000;

  const allDurations: number[] = [];
  const bucketSize = 10_000; // 10-second buckets
  const buckets = new Map<number, number[]>();

  let sent = 0;
  let failed = 0;

  while (Date.now() < endTime) {
    const sendTime = Date.now();
    sent++;

    // Fire and don't await — open-loop
    (async () => {
      try {
        const { downloadUrl, symmetricKey } = await uploadEncrypted(content);
        const t0 = Date.now();
        const encResp = await client.verifyRaw(session.gatewayUrl, {
          download_url: downloadUrl,
          processor_ids: ["core-c2pa"],
        });
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        const ms = Date.now() - t0;
        allDurations.push(ms);

        const bucketKey = Math.floor((sendTime - startTime) / bucketSize) * bucketSize;
        if (!buckets.has(bucketKey)) buckets.set(bucketKey, []);
        buckets.get(bucketKey)!.push(ms);
      } catch {
        failed++;
      }
    })();

    // Wait for next send
    const elapsed = Date.now() - sendTime;
    const sleepMs = Math.max(0, intervalMs - elapsed);
    if (sleepMs > 0) await new Promise(r => setTimeout(r, sleepMs));
  }

  // Wait for in-flight requests
  console.log("Waiting for in-flight requests...\n");
  await new Promise(r => setTimeout(r, 15_000));

  // Results
  console.log(`Sent: ${sent}, Succeeded: ${allDurations.length}, Failed: ${failed}\n`);

  if (allDurations.length > 0) {
    const overall = calcStats(allDurations);
    console.log(`Overall: avg=${overall.avg}ms p50=${overall.p50}ms p95=${overall.p95}ms p99=${overall.p99}ms\n`);

    // Per-bucket breakdown (detect drift)
    console.log("Time(s) | Count | Avg(ms) | p95(ms)");
    console.log("-".repeat(40));
    const sortedBuckets = [...buckets.entries()].sort((a, b) => a[0] - b[0]);
    for (const [offset, durations] of sortedBuckets) {
      const s = calcStats(durations);
      const timeSec = Math.round(offset / 1000);
      console.log(
        `${String(timeSec).padStart(7)} | ${String(s.count).padStart(5)} | ${String(s.avg).padStart(7)} | ${String(s.p95).padStart(7)}`
      );
    }

    // Drift detection
    if (sortedBuckets.length >= 3) {
      const firstAvg = calcStats(sortedBuckets[0][1]).avg;
      const lastAvg = calcStats(sortedBuckets[sortedBuckets.length - 1][1]).avg;
      const drift = ((lastAvg - firstAvg) / firstAvg * 100).toFixed(1);
      console.log(`\nLatency drift: ${drift}% (first bucket avg=${firstAvg}ms → last bucket avg=${lastAvg}ms)`);
      if (Math.abs(lastAvg - firstAvg) > firstAvg * 0.5) {
        console.log("⚠ Significant latency drift detected — possible memory leak or resource exhaustion");
      }
    }
  }
}

main().catch(console.error);
