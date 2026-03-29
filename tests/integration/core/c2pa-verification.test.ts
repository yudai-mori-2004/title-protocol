// SPDX-License-Identifier: Apache-2.0

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext, OWNER_WALLET } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertCoreResult } from "../helpers/assertions.ts";
import { ALL_C2PA_SIGNED, UNSIGNED, GOOGLE_SIGNED } from "../helpers/fixtures.ts";

describe("core-c2pa verification", () => {
  before(async () => { await getTestContext(); });

  // All C2PA-signed content — every format, every size
  describe("C2PA-signed content (all formats)", () => {
    for (const f of ALL_C2PA_SIGNED) {
      it(`verifies ${f.name}`, { timeout: 120_000 }, async () => {
        const r = await verifyContent(f.path, ["core-c2pa"]);
        assert.ok(r.core, "core result must exist");
        assertCoreResult(r.core, { wallet: OWNER_WALLET });
      });
    }
  });

  // Unsigned content — must be rejected
  describe("unsigned content rejection", () => {
    for (const f of UNSIGNED) {
      it(`rejects ${f.name}`, { timeout: 60_000 }, async () => {
        await assert.rejects(
          () => verifyContent(f.path, ["core-c2pa"]),
          (err: any) => {
            const msg = err.message || String(err);
            assert.ok(
              msg.includes("JUMBF") || msg.includes("Active Manifest") ||
              msg.includes("C2PA") || msg.includes("422"),
              `Expected C2PA rejection, got: ${msg}`,
            );
            return true;
          },
        );
      });
    }
  });
});
