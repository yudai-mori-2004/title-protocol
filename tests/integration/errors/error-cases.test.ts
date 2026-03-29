// SPDX-License-Identifier: Apache-2.0

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { UNSIGNED } from "../helpers/fixtures.ts";

describe("error cases", () => {
  before(async () => { await getTestContext(); });

  // Every unsigned format → rejected
  describe("unsigned content rejected by core-c2pa", () => {
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

  // Unsigned + extension → entire request fails (all-or-nothing)
  it("unsigned content fails even with image-pdq", { timeout: 60_000 }, async () => {
    await assert.rejects(
      () => verifyContent(UNSIGNED[0].path, ["core-c2pa", "image-pdq"]),
      (err: any) => {
        const msg = err.message || String(err);
        assert.ok(msg.includes("422") || msg.includes("JUMBF") || msg.includes("Active Manifest"));
        return true;
      },
    );
  });

  // Large unsigned content — ensures rejection isn't size-dependent
  it("rejects large unsigned JPEG (1080p)", { timeout: 60_000 }, async () => {
    await assert.rejects(
      () => verifyContent("tests/fixtures/c2pa/unsigned/jpeg-1080p.jpg", ["core-c2pa"]),
      (err: any) => {
        const msg = err.message || String(err);
        assert.ok(msg.includes("422") || msg.includes("JUMBF") || msg.includes("Active Manifest"));
        return true;
      },
    );
  });
});
