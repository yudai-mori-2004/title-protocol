// SPDX-License-Identifier: Apache-2.0

/**
 * Multi-processor tests.
 *
 * Verifies that multiple processors run correctly in a single request
 * and that the all-or-nothing failure model works.
 */

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext, OWNER_WALLET } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import {
  assertCoreResult,
  assertExtensionResult,
  assertPdqResult,
  assertCertResult,
} from "../helpers/assertions.ts";

describe("multi-processor", () => {
  before(async () => {
    await getTestContext();
  });

  it("core + image-pdq on same content", { timeout: 60_000 }, async () => {
    const result = await verifyContent(
      "tests/fixtures/images/jpeg/pixel_plane.jpg",
      ["core-c2pa", "image-pdq"],
    );

    assert.ok(result.core, "core result must exist");
    assertCoreResult(result.core, { wallet: OWNER_WALLET });

    const pdq = result.extensions.get("image-pdq");
    assert.ok(pdq, "image-pdq result must exist");
    assertExtensionResult(pdq, { extensionId: "image-pdq" });
    assertPdqResult(pdq);

    // content_hash must match across processors
    const coreHash = (result.core.payload as any).content_hash;
    const extHash = (pdq.payload as any).content_hash;
    assert.equal(coreHash, extHash, "content_hash must match across processors");
  });

  it("core + image-pdq + cert-google (three processors)", { timeout: 60_000 }, async () => {
    const result = await verifyContent(
      "tests/fixtures/images/jpeg/pixel_plane.jpg",
      ["core-c2pa", "image-pdq", "cert-google"],
    );

    assert.ok(result.core);
    assertCoreResult(result.core);
    assertPdqResult(result.extensions.get("image-pdq")!);
    assertCertResult(result.extensions.get("cert-google")!, {
      verified: true,
      rootCa: "Google C2PA Root CA G3",
    });
  });

  it("all-or-nothing: unsigned content fails entire request", { timeout: 60_000 }, async () => {
    await assert.rejects(
      () => verifyContent(
        "tests/fixtures/c2pa/unsigned/sample.jpg",
        ["core-c2pa", "image-pdq"],
      ),
      (err: any) => {
        const msg = err.message || String(err);
        assert.ok(
          msg.includes("422") || msg.includes("JUMBF") || msg.includes("Active Manifest"),
          `Expected 422 or C2PA error, got: ${msg}`,
        );
        return true;
      },
    );
  });
});
