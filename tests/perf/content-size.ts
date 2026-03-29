#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Content size scaling test.
 *
 * Measures how latency scales with content size.
 * Separates transfer-bound (upload) from compute-bound (verify) phases.
 *
 * Usage: npx tsx tests/perf/content-size.ts
 */

import {
  init, loadFixture, uploadEncrypted, calcStats, getClient, getSession,
} from "./helpers.ts";
import { decryptResponse } from "@title-protocol/sdk";

const REPEATS = 3;

const SIZE_FIXTURES = [
  { name: "JPEG 4x4", path: "tests/fixtures/c2pa/signed/sample.jpg", category: "image" },
  { name: "JPEG 640x480", path: "tests/fixtures/c2pa/signed/jpeg-640x480.jpg", category: "image" },
  { name: "JPEG 1080p", path: "tests/fixtures/c2pa/signed/jpeg-1080p.jpg", category: "image" },
  { name: "WAV 1s", path: "tests/fixtures/c2pa/signed/sample.wav", category: "audio" },
  { name: "WAV 5s", path: "tests/fixtures/c2pa/signed/wav-5s.wav", category: "audio" },
  { name: "MP3 3s", path: "tests/fixtures/c2pa/signed/sample.mp3", category: "audio" },
  { name: "MP4 1s 64x64", path: "tests/fixtures/c2pa/signed/mp4-1s-64x64.mp4", category: "video" },
  { name: "MP4 5s 640x480", path: "tests/fixtures/c2pa/signed/mp4-5s-640x480.mp4", category: "video" },
  { name: "MP4 10s 720p", path: "tests/fixtures/c2pa/signed/mp4-10s-720p.mp4", category: "video" },
];

async function main() {
  await init();
  const client = getClient();
  const session = getSession();

  console.log("=== Content Size Scaling Test ===\n");
  console.log(`${REPEATS} repeats per fixture, core-c2pa only\n`);
  console.log("Content           |  Size(KB) | Upload(ms) | Verify(ms) | Total(ms)");
  console.log("-".repeat(75));

  for (const fixture of SIZE_FIXTURES) {
    const content = loadFixture(fixture.path);
    const sizeKb = (content.length / 1024).toFixed(0);
    const uploadTimes: number[] = [];
    const verifyTimes: number[] = [];

    for (let i = 0; i < REPEATS; i++) {
      try {
        // Measure upload separately
        const t0 = Date.now();
        const { downloadUrl, symmetricKey } = await uploadEncrypted(content);
        const uploadMs = Date.now() - t0;
        uploadTimes.push(uploadMs);

        // Measure verify separately
        const t1 = Date.now();
        const encResp = await client.verifyRaw(session.gatewayUrl, {
          download_url: downloadUrl,
          processor_ids: ["core-c2pa"],
        });
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        const verifyMs = Date.now() - t1;
        verifyTimes.push(verifyMs);
      } catch (e: any) {
        console.log(`  FAIL: ${fixture.name} — ${e.message?.slice(0, 60)}`);
      }
    }

    if (uploadTimes.length > 0) {
      const uAvg = calcStats(uploadTimes).avg;
      const vAvg = calcStats(verifyTimes).avg;
      console.log(
        `${fixture.name.padEnd(18)}| ${sizeKb.padStart(9)} ` +
        `| ${String(uAvg).padStart(10)} ` +
        `| ${String(vAvg).padStart(10)} ` +
        `| ${String(uAvg + vAvg).padStart(9)}`
      );
    }
  }

  console.log("\nIf Verify(ms) scales linearly with Size(KB), the bottleneck is transfer/decrypt.");
  console.log("If Verify(ms) is constant regardless of size, the bottleneck is C2PA parsing.");
}

main().catch(console.error);
