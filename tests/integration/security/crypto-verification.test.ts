// SPDX-License-Identifier: Apache-2.0

/**
 * Security tests — cryptographic verification.
 *
 * Verifies the core trust property: tee_signature is a valid Ed25519
 * signature over {payload, attributes} using the tee_pubkey.
 * Also verifies cross-processor content_hash consistency.
 */

import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import { getTestContext } from "../helpers/setup.ts";
import { verifyContent } from "../helpers/verify.ts";
import { verifyTeeSignature } from "../helpers/assertions.ts";

const FIXTURE = "tests/fixtures/images/jpeg/pixel_plane.jpg";

describe("cryptographic verification", () => {
  before(async () => {
    await getTestContext();
  });

  it("core signed_json has valid Ed25519 signature", { timeout: 60_000 }, async () => {
    const result = await verifyContent(FIXTURE, ["core-c2pa"]);
    // verifyTeeSignature is called inside assertSignedJsonEnvelope,
    // but we also call it explicitly here as a dedicated security test.
    verifyTeeSignature(result.core!);
  });

  it("extension signed_json has valid Ed25519 signature", { timeout: 60_000 }, async () => {
    const result = await verifyContent(FIXTURE, ["core-c2pa", "image-pdq"]);
    verifyTeeSignature(result.extensions.get("image-pdq")!);
  });

  it("core and extension use same tee_pubkey", { timeout: 60_000 }, async () => {
    const result = await verifyContent(FIXTURE, ["core-c2pa", "image-pdq"]);

    const corePubkey = result.core!.tee_pubkey;
    const extPubkey = result.extensions.get("image-pdq")!.tee_pubkey;
    assert.equal(corePubkey, extPubkey, "TEE pubkey must be consistent across processors");
  });

  it("content_hash is consistent between core and all extensions", { timeout: 60_000 }, async () => {
    const result = await verifyContent(FIXTURE, ["core-c2pa", "image-pdq", "cert-google"]);

    const coreHash = (result.core!.payload as any).content_hash;
    const pdqHash = (result.extensions.get("image-pdq")!.payload as any).content_hash;
    const certHash = (result.extensions.get("cert-google")!.payload as any).content_hash;

    assert.equal(coreHash, pdqHash, "PDQ content_hash must match core");
    assert.equal(coreHash, certHash, "cert content_hash must match core");
  });
});
