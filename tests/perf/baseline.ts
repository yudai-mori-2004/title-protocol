#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Baseline latency measurement.
 *
 * Measures single-request latency for each processor and content size.
 * This is the foundation — all other perf tests compare against this.
 *
 * Usage: npx tsx tests/perf/baseline.ts
 */

import {
  init, timedVerify, loadFixture, calcStats, formatStats,
} from "./helpers.ts";

const FIXTURES = [
  { name: "JPEG 4x4 (13KB)", path: "tests/fixtures/c2pa/signed/sample.jpg" },
  { name: "JPEG 640x480 (30KB)", path: "tests/fixtures/c2pa/signed/jpeg-640x480.jpg" },
  { name: "JPEG 1080p (215KB)", path: "tests/fixtures/c2pa/signed/jpeg-1080p.jpg" },
  { name: "PNG (13KB)", path: "tests/fixtures/c2pa/signed/sample.png" },
  { name: "TIFF (13KB)", path: "tests/fixtures/c2pa/signed/sample.tiff" },
  { name: "WAV 1s (29KB)", path: "tests/fixtures/c2pa/signed/sample.wav" },
  { name: "MP3 3s (62KB)", path: "tests/fixtures/c2pa/signed/sample.mp3" },
  { name: "MP4 1s (16KB)", path: "tests/fixtures/c2pa/signed/mp4-1s-64x64.mp4" },
  { name: "MP4 5s (96KB)", path: "tests/fixtures/c2pa/signed/mp4-5s-640x480.mp4" },
  { name: "MP4 10s (257KB)", path: "tests/fixtures/c2pa/signed/mp4-10s-720p.mp4" },
];

const PROCESSOR_SETS = [
  { name: "core-c2pa only", ids: ["core-c2pa"] },
  { name: "core + image-pdq", ids: ["core-c2pa", "image-pdq"] },
  { name: "core + cert-google", ids: ["core-c2pa", "cert-google"] },
  { name: "core + video-vpdq", ids: ["core-c2pa", "video-vpdq"] },
  { name: "core + pdq + cert-google", ids: ["core-c2pa", "image-pdq", "cert-google"] },
  { name: "all 7 processors", ids: ["core-c2pa", "image-pdq", "image-phash", "video-vpdq", "cert-google", "cert-sony", "cert-leica"] },
];

const REPEATS = 5;

async function main() {
  await init();

  console.log("=== Baseline: Content Size × Latency ===\n");
  console.log(`Each measurement: ${REPEATS} repeats, core-c2pa only\n`);

  for (const fixture of FIXTURES) {
    const content = loadFixture(fixture.path);
    const durations: number[] = [];

    for (let i = 0; i < REPEATS; i++) {
      try {
        const { durationMs } = await timedVerify(content);
        durations.push(durationMs);
      } catch (e: any) {
        console.log(`  FAIL: ${fixture.name} — ${e.message}`);
      }
    }

    if (durations.length > 0) {
      console.log(formatStats(fixture.name, calcStats(durations)));
    }
  }

  console.log("\n=== Baseline: Processor Count × Latency ===\n");
  console.log("Using JPEG 640x480 as standard content\n");

  const stdContent = loadFixture("tests/fixtures/c2pa/signed/jpeg-640x480.jpg");

  for (const ps of PROCESSOR_SETS) {
    const durations: number[] = [];
    for (let i = 0; i < REPEATS; i++) {
      try {
        const { durationMs } = await timedVerify(stdContent, ps.ids);
        durations.push(durationMs);
      } catch (e: any) {
        console.log(`  FAIL: ${ps.name} — ${e.message}`);
      }
    }

    if (durations.length > 0) {
      console.log(formatStats(ps.name, calcStats(durations)));
    }
  }
}

main().catch(console.error);
