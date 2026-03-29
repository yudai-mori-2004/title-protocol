// SPDX-License-Identifier: Apache-2.0

/**
 * content_hash determinism tests.
 *
 * content_hash = SHA-256(COSE_Sign1 signature bytes) from the C2PA manifest.
 * Same file must always produce the same content_hash. Extensions must
 * produce the same content_hash as core for the same content.
 */

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertContentHash } from "../helpers/assertions.ts";

const FIXTURE = "tests/fixtures/images/jpeg/pixel_plane.jpg";

describe("content_hash determinism", () => {
  before(async () => {
    await getTestContext();
  });

  it("same file produces identical content_hash", { timeout: 120_000 }, async () => {
    const r1 = await verifyContent(FIXTURE, ["core-c2pa"]);
    const r2 = await verifyContent(FIXTURE, ["core-c2pa"]);

    const h1 = (r1.core!.payload as any).content_hash;
    const h2 = (r2.core!.payload as any).content_hash;

    assertContentHash(h1);
    assertContentHash(h2);
    assert.equal(h1, h2, "Same file must produce identical content_hash");
  });

  it("content_hash is the same regardless of processor set", { timeout: 120_000 }, async () => {
    const r1 = await verifyContent(FIXTURE, ["core-c2pa"]);
    const r2 = await verifyContent(FIXTURE, ["core-c2pa", "image-pdq"]);

    const coreHash = (r1.core!.payload as any).content_hash;
    const coreHash2 = (r2.core!.payload as any).content_hash;
    assert.equal(coreHash, coreHash2, "core content_hash must not depend on processor set");
  });

  it("extension content_hash matches core content_hash", { timeout: 60_000 }, async () => {
    const result = await verifyContent(FIXTURE, ["core-c2pa", "image-pdq"]);

    const coreHash = (result.core!.payload as any).content_hash;
    const extHash = (result.extensions.get("image-pdq")!.payload as any).content_hash;

    assert.equal(coreHash, extHash, "Extension content_hash must match core");
  });
});
