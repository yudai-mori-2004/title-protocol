#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Content size scaling test.
 *
 * Measures how latency scales with content size using MP4 files
 * from 1MB to 500MB. Separates upload from verify phases.
 *
 * Usage: npx tsx tests/perf/content-size.ts
 */

import * as fs from "node:fs";
import {
  init, loadFixture, uploadEncrypted, calcStats, getClient, getSession,
} from "./helpers.ts";
import { decryptResponse } from "@title-protocol/sdk";

const REPEATS = 3;

const SIZE_FIXTURES = [
  { name: "MP4 1MB", path: "tests/fixtures/c2pa/signed/mp4-1mb.mp4" },
  { name: "MP4 5MB", path: "tests/fixtures/c2pa/signed/mp4-5mb.mp4" },
  { name: "MP4 10MB", path: "tests/fixtures/c2pa/signed/mp4-10mb.mp4" },
  { name: "MP4 25MB", path: "tests/fixtures/c2pa/signed/mp4-25mb.mp4" },
  { name: "MP4 50MB", path: "tests/fixtures/c2pa/signed/mp4-50mb.mp4" },
  { name: "MP4 100MB", path: "tests/fixtures/c2pa/signed/mp4-100mb.mp4" },
  { name: "MP4 200MB", path: "tests/fixtures/c2pa/signed/mp4-200mb.mp4" },
  { name: "MP4 500MB", path: "tests/fixtures/c2pa/signed/mp4-500mb.mp4" },
];

async function main() {
  await init();
  const client = getClient();
  const session = getSession();

  console.log("=== Content Size Scaling (MP4, core-c2pa only) ===\n");
  console.log(`${REPEATS} repeats per size\n`);
  console.log("Size       | File(MB) | Upload(ms) | Verify(ms) | Total(ms) | Status");
  console.log("-".repeat(75));

  for (const fixture of SIZE_FIXTURES) {
    let content: Uint8Array;
    try {
      content = loadFixture(fixture.path);
    } catch {
      console.log(`${fixture.name.padEnd(11)}| ${"-".padStart(8)} | ${"n/a".padStart(10)} | ${"n/a".padStart(10)} | ${"n/a".padStart(9)} | file not found`);
      continue;
    }

    const fileMb = (content.length / 1024 / 1024).toFixed(1);
    const uploadTimes: number[] = [];
    const verifyTimes: number[] = [];
    let status = "OK";

    for (let i = 0; i < REPEATS; i++) {
      try {
        const t0 = Date.now();
        const { downloadUrl, symmetricKey } = await uploadEncrypted(content);
        const uploadMs = Date.now() - t0;
        uploadTimes.push(uploadMs);

        const t1 = Date.now();
        const encResp = await client.verifyRaw(session.gatewayUrl, {
          download_url: downloadUrl,
          processor_ids: ["core-c2pa"],
        });
        await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
        const verifyMs = Date.now() - t1;
        verifyTimes.push(verifyMs);
      } catch (e: any) {
        const msg = e.message?.slice(0, 40) ?? "unknown";
        status = `FAIL: ${msg}`;
        break;
      }
    }

    if (uploadTimes.length > 0 && verifyTimes.length > 0) {
      const uAvg = calcStats(uploadTimes).avg;
      const vAvg = calcStats(verifyTimes).avg;
      console.log(
        `${fixture.name.padEnd(11)}| ${fileMb.padStart(8)} ` +
        `| ${String(uAvg).padStart(10)} ` +
        `| ${String(vAvg).padStart(10)} ` +
        `| ${String(uAvg + vAvg).padStart(9)} ` +
        `| ${status}`
      );
    } else {
      console.log(
        `${fixture.name.padEnd(11)}| ${fileMb.padStart(8)} ` +
        `| ${"—".padStart(10)} ` +
        `| ${"—".padStart(10)} ` +
        `| ${"—".padStart(9)} ` +
        `| ${status}`
      );
    }
  }
}

main().catch(console.error);
