// SPDX-License-Identifier: Apache-2.0

/**
 * Assertion helpers for signed_json validation.
 *
 * Every response is validated for:
 * 1. Envelope fields (protocol, tee_type, tee_pubkey, tee_signature, tee_attestation)
 * 2. Ed25519 cryptographic signature verification (core trust property)
 * 3. Payload structure (core vs extension vs processor-specific)
 */

import assert from "node:assert/strict";
import { ed25519 } from "@noble/curves/ed25519";
import bs58 from "bs58";
import type { SignedJson } from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// Ed25519 signature verification
// ---------------------------------------------------------------------------

/**
 * Verify tee_signature against tee_pubkey using Ed25519.
 *
 * The TEE signs: serde_json::to_vec({"payload": ..., "attributes": ...})
 * We reconstruct this sign_target from the signed_json fields.
 */
export function verifyTeeSignature(sj: SignedJson): void {
  const { tee_pubkey, tee_signature, payload, attributes } = sj;

  // Reconstruct sign_target exactly as Rust produces it
  const signTarget = JSON.stringify({ payload, attributes });
  const messageBytes = new TextEncoder().encode(signTarget);

  const pubkeyBytes = bs58.decode(tee_pubkey);
  const signatureBytes = Buffer.from(tee_signature, "base64");

  const valid = ed25519.verify(signatureBytes, messageBytes, pubkeyBytes);
  assert.ok(valid, `Ed25519 signature verification failed for tee_pubkey=${tee_pubkey}`);
}

// ---------------------------------------------------------------------------
// Envelope (common to all signed_json)
// ---------------------------------------------------------------------------

export function assertSignedJsonEnvelope(sj: SignedJson): void {
  assert.ok(sj, "signed_json must exist");
  assert.ok(sj.tee_type, "tee_type must be present");
  assert.ok(sj.tee_pubkey, "tee_pubkey must be present");
  assert.ok(sj.tee_signature, "tee_signature must be present");
  assert.ok(sj.tee_attestation, "tee_attestation must be present");
  assert.ok(Array.isArray(sj.attributes), "attributes must be an array");
  assert.ok(sj.payload, "payload must be present");

  verifyTeeSignature(sj);
}

// ---------------------------------------------------------------------------
// content_hash format
// ---------------------------------------------------------------------------

export function assertContentHash(hash: string): void {
  assert.ok(hash, "content_hash must be present");
  assert.match(hash, /^0x[0-9a-f]{64}$/, `Invalid content_hash: ${hash}`);
}

// ---------------------------------------------------------------------------
// Core result (core-c2pa)
// ---------------------------------------------------------------------------

export function assertCoreResult(
  sj: SignedJson,
  opts?: { wallet?: string; contentType?: string }
): void {
  assertSignedJsonEnvelope(sj);
  assert.equal(sj.protocol, "Title-v1");

  const p = sj.payload as any;
  assertContentHash(p.content_hash);
  assert.ok(p.content_type, "content_type must be present");
  assert.ok(p.creator_wallet, "creator_wallet must be present");
  assert.ok(Array.isArray(p.nodes), "nodes must be an array");
  assert.ok(Array.isArray(p.links), "links must be an array");

  if (opts?.wallet) assert.equal(p.creator_wallet, opts.wallet);
  if (opts?.contentType) assert.equal(p.content_type, opts.contentType);
}

// ---------------------------------------------------------------------------
// Extension result (any extension)
// ---------------------------------------------------------------------------

export function assertExtensionResult(
  sj: SignedJson,
  opts: { extensionId: string; contentType?: string }
): void {
  assertSignedJsonEnvelope(sj);
  assert.equal(sj.protocol, "Title-Extension-v1");

  const p = sj.payload as any;
  assertContentHash(p.content_hash);
  assert.ok(p.content_type, "content_type must be present");
  assert.ok(p.creator_wallet, "creator_wallet must be present");
  assert.equal(p.extension_id, opts.extensionId);
  assert.ok(p.wasm_hash, "wasm_hash must be present");
  assert.ok(p.wasm_source, "wasm_source must be present");

  if (opts.contentType) assert.equal(p.content_type, opts.contentType);
}

// ---------------------------------------------------------------------------
// Processor-specific assertions
// ---------------------------------------------------------------------------

/** Assert image-pdq output fields. */
export function assertPdqResult(sj: SignedJson): void {
  const p = sj.payload as any;
  assert.match(p.pdqhash, /^[0-9a-f]{64}$/, `Invalid pdqhash: ${p.pdqhash}`);
  assert.ok(typeof p.quality === "number", "quality must be a number");
  assert.ok(p.quality >= 0 && p.quality <= 100, `quality out of range: ${p.quality}`);
  assert.equal(p.algorithm, "pdq");
  assert.equal(p.bits, 256);
}

/** Assert video-vpdq output fields. */
export function assertVpdqResult(sj: SignedJson): void {
  const p = sj.payload as any;
  assert.ok(Array.isArray(p.frames), "frames must be an array");
  assert.ok(p.frames.length > 0, "frames must not be empty");
  assert.ok(typeof p.frame_count === "number", "frame_count must be a number");
  assert.equal(p.algorithm, "vpdq");
  assert.equal(p.sampling_fps, 1);

  for (const frame of p.frames) {
    assert.match(frame.pdqhash, /^[0-9a-f]{64}$/, `Invalid frame pdqhash`);
    assert.ok(typeof frame.quality === "number", "frame quality must be a number");
    assert.ok(typeof frame.timestamp === "number", "frame timestamp must be a number");
  }
}

/** Assert cert-* output fields. */
export function assertCertResult(
  sj: SignedJson,
  opts: { verified: boolean; rootCa?: string }
): void {
  const p = sj.payload as any;
  assert.equal(p.verified, opts.verified, `Expected verified=${opts.verified}`);
  assert.ok(Array.isArray(p.chain), "chain must be an array");
  assert.ok(p.root_ca, "root_ca must be present");
  assert.ok(p.root_spki, "root_spki must be present");

  if (opts.rootCa) assert.equal(p.root_ca, opts.rootCa);
}
