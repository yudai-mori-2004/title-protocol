// SPDX-License-Identifier: Apache-2.0

/**
 * cert-sony, cert-leica, cert-rootlens — vendor cert chain verification.
 *
 * No real vendor-signed fixtures exist yet (only Google Pixel photos).
 * Negative test: Google-signed content should return verified=false for
 * all non-Google cert processors.
 */

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertExtensionResult, assertCertResult } from "../helpers/assertions.ts";

const GOOGLE_FIXTURE = "tests/fixtures/images/jpeg/pixel_plane.jpg";

const VENDORS = [
  { id: "cert-sony", rootCa: "SONY C2PA Root CA G2" },
  { id: "cert-leica", rootCa: "Leica C2PA Root CA" },
  { id: "cert-rootlens", rootCa: "RootLens Root CA" },
];

describe("cert vendor processors", () => {
  before(async () => {
    await getTestContext();
  });

  for (const vendor of VENDORS) {
    it(`${vendor.id} returns verified=false for Google content`, { timeout: 60_000 }, async () => {
      const result = await verifyContent(GOOGLE_FIXTURE, ["core-c2pa", vendor.id]);

      const sj = result.extensions.get(vendor.id);
      assert.ok(sj, `${vendor.id} result must exist`);
      assertExtensionResult(sj, { extensionId: vendor.id });
      assertCertResult(sj, { verified: false, rootCa: vendor.rootCa });
    });
  }
});
