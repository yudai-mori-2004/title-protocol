// SPDX-License-Identifier: Apache-2.0

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertExtensionResult, assertPdqResult } from "../helpers/assertions.ts";
import { PDQ_IMAGES, GOOGLE_SIGNED } from "../helpers/fixtures.ts";

describe("image-pdq", () => {
  before(async () => { await getTestContext(); });

  // Format breadth — every C2PA-signed image format + size
  describe("format breadth", () => {
    for (const f of PDQ_IMAGES) {
      it(`hashes ${f.name}`, { timeout: 60_000 }, async () => {
        const r = await verifyContent(f.path, ["core-c2pa", "image-pdq"]);
        const sj = r.extensions.get("image-pdq");
        assert.ok(sj, "image-pdq result must exist");
        assertExtensionResult(sj, { extensionId: "image-pdq" });
        assertPdqResult(sj);
      });
    }
  });

  // Determinism
  it("same content produces identical hash", { timeout: 120_000 }, async () => {
    const f = GOOGLE_SIGNED[0];
    const r1 = await verifyContent(f.path, ["core-c2pa", "image-pdq"]);
    const r2 = await verifyContent(f.path, ["core-c2pa", "image-pdq"]);
    const h1 = (r1.extensions.get("image-pdq")!.payload as any).pdqhash;
    const h2 = (r2.extensions.get("image-pdq")!.payload as any).pdqhash;
    assert.equal(h1, h2, "Same content must produce identical PDQ hash");
  });

  // Different images produce different hashes
  it("different images produce different hashes", { timeout: 120_000 }, async () => {
    const r1 = await verifyContent(GOOGLE_SIGNED[0].path, ["core-c2pa", "image-pdq"]);
    const r2 = await verifyContent(GOOGLE_SIGNED[1].path, ["core-c2pa", "image-pdq"]);
    const h1 = (r1.extensions.get("image-pdq")!.payload as any).pdqhash;
    const h2 = (r2.extensions.get("image-pdq")!.payload as any).pdqhash;
    assert.notEqual(h1, h2, "Different images should produce different hashes");
  });
});
