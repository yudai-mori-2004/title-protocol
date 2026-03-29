// SPDX-License-Identifier: Apache-2.0

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertExtensionResult, assertCertResult } from "../helpers/assertions.ts";
import { GOOGLE_SIGNED, C2PA_SIGNED_IMAGES } from "../helpers/fixtures.ts";

describe("cert-google", () => {
  before(async () => { await getTestContext(); });

  // Positive: Google-signed → verified: true
  for (const f of GOOGLE_SIGNED) {
    it(`verifies ${f.name} as Google-signed`, { timeout: 60_000 }, async () => {
      const r = await verifyContent(f.path, ["core-c2pa", "cert-google"]);
      const sj = r.extensions.get("cert-google");
      assert.ok(sj, "cert-google result must exist");
      assertExtensionResult(sj, { extensionId: "cert-google" });
      assertCertResult(sj, { verified: true, rootCa: "Google C2PA Root CA G3" });
      assert.ok((sj.payload as any).chain.length > 0, "chain must not be empty");
    });
  }

  // Negative: self-signed → verified: false (not an error)
  it("returns verified=false for self-signed content", { timeout: 60_000 }, async () => {
    const r = await verifyContent(C2PA_SIGNED_IMAGES[0].path, ["core-c2pa", "cert-google"]);
    const sj = r.extensions.get("cert-google");
    assert.ok(sj, "cert-google result must exist");
    assertExtensionResult(sj, { extensionId: "cert-google" });
    assertCertResult(sj, { verified: false });
  });
});
