// SPDX-License-Identifier: Apache-2.0

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertExtensionResult, assertVpdqResult } from "../helpers/assertions.ts";
import { C2PA_SIGNED_VIDEO } from "../helpers/fixtures.ts";

describe("video-vpdq", () => {
  before(async () => { await getTestContext(); });

  // Format breadth — every video size/duration
  describe("format breadth", () => {
    for (const f of C2PA_SIGNED_VIDEO) {
      it(`hashes ${f.name}`, { timeout: 180_000 }, async () => {
        const r = await verifyContent(f.path, ["core-c2pa", "video-vpdq"]);
        const sj = r.extensions.get("video-vpdq");
        assert.ok(sj, "video-vpdq result must exist");
        assertExtensionResult(sj, { extensionId: "video-vpdq" });
        assertVpdqResult(sj);

        // Timestamps monotonically increasing
        const frames = (sj.payload as any).frames;
        for (let i = 1; i < frames.length; i++) {
          assert.ok(frames[i].timestamp >= frames[i - 1].timestamp, "Timestamps must be monotonic");
        }
      });
    }
  });

  // Determinism
  it("same video produces identical frame hashes", { timeout: 180_000 }, async () => {
    const f = C2PA_SIGNED_VIDEO[0];
    const r1 = await verifyContent(f.path, ["core-c2pa", "video-vpdq"]);
    const r2 = await verifyContent(f.path, ["core-c2pa", "video-vpdq"]);
    const f1 = (r1.extensions.get("video-vpdq")!.payload as any).frames;
    const f2 = (r2.extensions.get("video-vpdq")!.payload as any).frames;
    assert.equal(f1.length, f2.length, "Frame count must be deterministic");
    for (let i = 0; i < f1.length; i++) {
      assert.equal(f1[i].pdqhash, f2[i].pdqhash, `Frame ${i} hash must be deterministic`);
    }
  });

  // Longer video produces more frames
  it("longer video produces more frames", { timeout: 180_000 }, async () => {
    const short = C2PA_SIGNED_VIDEO[0]; // 1s
    const long = C2PA_SIGNED_VIDEO[1]; // 5s
    const r1 = await verifyContent(short.path, ["core-c2pa", "video-vpdq"]);
    const r2 = await verifyContent(long.path, ["core-c2pa", "video-vpdq"]);
    const c1 = (r1.extensions.get("video-vpdq")!.payload as any).frame_count;
    const c2 = (r2.extensions.get("video-vpdq")!.payload as any).frame_count;
    assert.ok(c2 > c1, `5s video (${c2} frames) should have more frames than 1s (${c1})`);
  });
});
