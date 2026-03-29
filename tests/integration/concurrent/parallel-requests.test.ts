// SPDX-License-Identifier: Apache-2.0

/**
 * Concurrent request tests.
 *
 * Verifies that parallel requests don't mix results or corrupt state.
 * Each request uses a different fixture so we can verify results
 * aren't swapped between requests.
 */

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertCoreResult, assertContentHash } from "../helpers/assertions.ts";

describe("concurrent requests", () => {
  before(async () => {
    await getTestContext();
  });

  it("two parallel requests produce correct independent results", { timeout: 120_000 }, async () => {
    const [r1, r2] = await Promise.all([
      verifyContent("tests/fixtures/images/jpeg/pixel_plane.jpg", ["core-c2pa"]),
      verifyContent("tests/fixtures/images/jpeg/pixel_ramen.jpg", ["core-c2pa"]),
    ]);

    assertCoreResult(r1.core!);
    assertCoreResult(r2.core!);

    const h1 = (r1.core!.payload as any).content_hash;
    const h2 = (r2.core!.payload as any).content_hash;

    assertContentHash(h1);
    assertContentHash(h2);
    assert.notEqual(h1, h2, "Different content must produce different hashes (no result mixing)");
  });

  it("four parallel requests all succeed", { timeout: 180_000 }, async () => {
    const fixture = "tests/fixtures/images/jpeg/pixel_plane.jpg";
    const results = await Promise.all([
      verifyContent(fixture, ["core-c2pa"]),
      verifyContent(fixture, ["core-c2pa"]),
      verifyContent(fixture, ["core-c2pa"]),
      verifyContent(fixture, ["core-c2pa"]),
    ]);

    const hashes = results.map(r => (r.core!.payload as any).content_hash);

    // All should produce the same hash (same content)
    for (const h of hashes) {
      assertContentHash(h);
      assert.equal(h, hashes[0], "Same content must produce same hash under concurrency");
    }
  });
});
