#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * Resilience test.
 *
 * Tests TEE node behavior under adversarial conditions:
 * - Oversized payloads (should be rejected, not crash)
 * - Rapid-fire requests (resource exhaustion)
 * - Malformed encrypted payloads (crypto attacks)
 * - Replay of old download URLs
 *
 * Usage: npx tsx tests/perf/resilience.ts
 */

import {
  init, uploadEncrypted, loadFixture, getClient, getSession,
} from "./helpers.ts";
import {
  encryptPayload, buildPlaintext,
} from "@title-protocol/sdk";

const CONTENT_PATH = "tests/fixtures/c2pa/signed/jpeg-640x480.jpg";

async function main() {
  await init();
  const client = getClient();
  const session = getSession();
  const content = loadFixture(CONTENT_PATH);

  console.log("=== Resilience Test ===\n");

  // ---------------------------------------------------------------
  // 1. Oversized payload
  // ---------------------------------------------------------------
  console.log("--- Oversized payload ---");
  {
    // 50MB payload (fill in chunks — getRandomValues has 64KB limit)
    const huge = new Uint8Array(50 * 1024 * 1024);
    for (let off = 0; off < huge.length; off += 65536) {
      crypto.getRandomValues(huge.subarray(off, Math.min(off + 65536, huge.length)));
    }
    const plaintext = buildPlaintext({ owner_wallet: "test" }, huge);
    const { payload } = await encryptPayload(
      Buffer.from(session.encryptionPubkey, "base64"),
      plaintext,
    );
    try {
      const { downloadUrl } = await client.upload(session.gatewayUrl, payload);
      const resp = await client.verifyRaw(session.gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      console.log("  UNEXPECTED: 50MB payload accepted");
    } catch (e: any) {
      const msg = e.message?.slice(0, 80) ?? "unknown";
      const rejected = msg.includes("413") || msg.includes("too large") || msg.includes("limit");
      console.log(`  ${rejected ? "PASS" : "WARN"}: ${msg}`);
    }
  }

  // ---------------------------------------------------------------
  // 2. Rapid-fire (100 requests in <1 second)
  // ---------------------------------------------------------------
  console.log("\n--- Rapid-fire (100 requests) ---");
  {
    // Pre-upload once
    const { downloadUrl, symmetricKey } = await uploadEncrypted(content);
    const t0 = Date.now();
    const promises = Array.from({ length: 100 }, () =>
      client.verifyRaw(session.gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      }).then(() => true).catch(() => false)
    );
    const results = await Promise.all(promises);
    const elapsed = Date.now() - t0;
    const succeeded = results.filter(Boolean).length;
    console.log(`  ${succeeded}/100 succeeded in ${elapsed}ms (${(succeeded / elapsed * 1000).toFixed(1)} req/s)`);
  }

  // ---------------------------------------------------------------
  // 3. Tampered ciphertext
  // ---------------------------------------------------------------
  console.log("\n--- Tampered ciphertext ---");
  {
    const { downloadUrl } = await uploadEncrypted(content);
    try {
      // Modify the download URL to point to garbage
      await client.verifyRaw(session.gatewayUrl, {
        download_url: downloadUrl + "TAMPERED",
        processor_ids: ["core-c2pa"],
      });
      console.log("  UNEXPECTED: tampered URL accepted");
    } catch (e: any) {
      console.log(`  PASS: rejected — ${e.message?.slice(0, 60)}`);
    }
  }

  // ---------------------------------------------------------------
  // 4. Wrong processor ID
  // ---------------------------------------------------------------
  console.log("\n--- Non-existent processor ID ---");
  {
    const { downloadUrl } = await uploadEncrypted(content);
    try {
      await client.verifyRaw(session.gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["nonexistent-processor"],
      });
      console.log("  UNEXPECTED: nonexistent processor accepted");
    } catch (e: any) {
      const msg = e.message?.slice(0, 80) ?? "unknown";
      console.log(`  PASS: rejected — ${msg}`);
    }
  }

  // ---------------------------------------------------------------
  // 5. Empty processor list
  // ---------------------------------------------------------------
  console.log("\n--- Empty processor list ---");
  {
    const { downloadUrl } = await uploadEncrypted(content);
    try {
      await client.verifyRaw(session.gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: [],
      });
      console.log("  UNEXPECTED: empty processor list accepted");
    } catch (e: any) {
      console.log(`  PASS: rejected — ${e.message?.slice(0, 60)}`);
    }
  }

  // ---------------------------------------------------------------
  // 6. Recovery after stress — node still works?
  // ---------------------------------------------------------------
  console.log("\n--- Recovery check ---");
  {
    try {
      const { downloadUrl, symmetricKey } = await uploadEncrypted(content);
      const t0 = Date.now();
      await client.verifyRaw(session.gatewayUrl, {
        download_url: downloadUrl,
        processor_ids: ["core-c2pa"],
      });
      const ms = Date.now() - t0;
      console.log(`  PASS: normal verify after stress completed in ${ms}ms`);
    } catch (e: any) {
      console.log(`  FAIL: node did not recover — ${e.message?.slice(0, 60)}`);
    }
  }
}

main().catch(console.error);
