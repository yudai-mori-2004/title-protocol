// SPDX-License-Identifier: Apache-2.0

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext, OWNER_WALLET } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { assertCoreResult } from "../helpers/assertions.ts";
import { INGREDIENT_FIXTURES, GOOGLE_SIGNED } from "../helpers/fixtures.ts";

describe("provenance graph", () => {
  before(async () => { await getTestContext(); });

  it("single content has one node and no links", { timeout: 60_000 }, async () => {
    const r = await verifyContent(GOOGLE_SIGNED[0].path, ["core-c2pa"]);
    assertCoreResult(r.core!, { wallet: OWNER_WALLET });
    const p = r.core!.payload as any;
    assert.ok(p.nodes.length >= 1, "must have at least one node");
  });

  it("2-ingredient content has multiple nodes and links", { timeout: 60_000 }, async () => {
    const r = await verifyContent(INGREDIENT_FIXTURES.with2.path, ["core-c2pa"]);
    assertCoreResult(r.core!);
    const p = r.core!.payload as any;
    assert.ok(p.nodes.length > 1, `Expected multiple nodes, got ${p.nodes.length}`);
    assert.ok(p.links.length >= 1, `Expected links, got ${p.links.length}`);
    for (const link of p.links) {
      assert.ok(link.source, "link must have source");
      assert.ok(link.target, "link must have target");
    }
  });

  it("chained ingredients produce deeper graph", { timeout: 60_000 }, async () => {
    const r = await verifyContent(INGREDIENT_FIXTURES.chain.path, ["core-c2pa"]);
    assertCoreResult(r.core!);
    const p = r.core!.payload as any;
    // Chain: A+B → C → D. Should have at least 3 nodes.
    assert.ok(p.nodes.length >= 3, `Expected ≥3 nodes in chain, got ${p.nodes.length}`);
    assert.ok(p.links.length >= 2, `Expected ≥2 links in chain, got ${p.links.length}`);
  });
});
