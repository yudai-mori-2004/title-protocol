// SPDX-License-Identifier: Apache-2.0

/**
 * chain.ts のユニットテスト
 *
 * 合成バッファで GlobalConfigAccount / TeeNodeAccount のデシリアライズをテスト。
 * Solana接続は不要（純粋なバイト列パース）。
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { PublicKey } from "@solana/web3.js";
import bs58 from "bs58";

import {
  findGlobalConfigPDA,
  findTeeNodePDA,
  findWasmModulePDA,
  TITLE_CONFIG_PROGRAM_ID,
  _deserializeGlobalConfig,
  _deserializeTeeNodeAccount,
  _deserializeWasmModuleAccount,
} from "../chain";

// ---------------------------------------------------------------------------
// Helpers: build synthetic Anchor account buffers
// ---------------------------------------------------------------------------

function anchorDiscriminator(accountName: string): Buffer {
  return createHash("sha256")
    .update(`account:${accountName}`)
    .digest()
    .subarray(0, 8);
}

function u32le(n: number): Buffer {
  const buf = Buffer.alloc(4);
  buf.writeUInt32LE(n);
  return buf;
}

function u64le(n: bigint): Buffer {
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64LE(n);
  return buf;
}

/** Borsh Option<u64>: 0x00 for None, 0x01 + 8-byte LE for Some */
function optionU64(val: number | undefined): Buffer {
  if (val === undefined) return Buffer.from([0x00]);
  return Buffer.concat([Buffer.from([0x01]), u64le(BigInt(val))]);
}

/** Build default ResourceLimitsOnChain (all None). */
function defaultResourceLimits(): Buffer {
  return Buffer.concat([
    optionU64(undefined),
    optionU64(undefined),
    optionU64(undefined),
    optionU64(undefined),
    optionU64(undefined),
    optionU64(undefined),
    optionU64(undefined),
  ]);
}

function borshString(s: string): Buffer {
  const encoded = Buffer.from(s, "utf-8");
  return Buffer.concat([u32le(encoded.length), encoded]);
}

/** Build a minimal GlobalConfigAccount buffer. */
function buildGlobalConfigBuffer(opts: {
  authority: Buffer;
  coreMint: Buffer;
  extMint: Buffer;
  nodeKeys: Buffer[];
  tsaKeys: Buffer[];
  wasmIds: Buffer[];
  resourceLimits?: Buffer;
}): Buffer {
  const parts: Buffer[] = [];

  parts.push(anchorDiscriminator("GlobalConfigAccount"));
  parts.push(opts.authority);
  parts.push(opts.coreMint);
  parts.push(opts.extMint);

  // Vec<[u8; 32]> trusted_node_keys
  parts.push(u32le(opts.nodeKeys.length));
  for (const k of opts.nodeKeys) parts.push(k);

  // Vec<[u8; 32]> trusted_tsa_keys
  parts.push(u32le(opts.tsaKeys.length));
  for (const k of opts.tsaKeys) parts.push(k);

  // Vec<[u8; 32]> trusted_wasm_ids
  parts.push(u32le(opts.wasmIds.length));
  for (const id of opts.wasmIds) {
    parts.push(id);
  }

  // ResourceLimitsOnChain
  parts.push(opts.resourceLimits ?? defaultResourceLimits());

  return Buffer.concat(parts);
}

/** Build a minimal TeeNodeAccount buffer. */
function buildTeeNodeBuffer(opts: {
  signingPubkey: Buffer;
  encryptionPubkey: Buffer;
  gatewayPubkey: Buffer;
  gatewayEndpoint: string;
  status: number;
  teeType: number;
  measurements: { key: Buffer; value: Buffer }[];
  bump: number;
}): Buffer {
  const parts: Buffer[] = [];

  parts.push(anchorDiscriminator("TeeNodeAccount"));
  parts.push(opts.signingPubkey);
  parts.push(opts.encryptionPubkey);
  parts.push(opts.gatewayPubkey);
  parts.push(borshString(opts.gatewayEndpoint));
  parts.push(Buffer.from([opts.status]));
  parts.push(Buffer.from([opts.teeType]));

  // Vec<MeasurementEntry>
  parts.push(u32le(opts.measurements.length));
  for (const m of opts.measurements) {
    parts.push(m.key);
    parts.push(m.value);
  }

  parts.push(Buffer.from([opts.bump]));
  return Buffer.concat(parts);
}

/** Random 32-byte buffer. */
function randomBytes32(): Buffer {
  const buf = Buffer.alloc(32);
  for (let i = 0; i < 32; i++) buf[i] = Math.floor(Math.random() * 256);
  return buf;
}

/** Build a 16-byte measurement key from a string (null-padded). */
function measurementKey(name: string): Buffer {
  const buf = Buffer.alloc(16);
  buf.write(name, "utf-8");
  return buf;
}

/** Build a 48-byte measurement value from hex. */
function measurementValue48(): Buffer {
  const buf = Buffer.alloc(48);
  for (let i = 0; i < 48; i++) buf[i] = i;
  return buf;
}

/** Build a 32-byte extension_id from string (null-padded). */
function extensionIdBytes(id: string): Buffer {
  const buf = Buffer.alloc(32);
  buf.write(id, "utf-8");
  return buf;
}

/** Build a minimal WasmModuleAccount buffer. */
function buildWasmModuleBuffer(opts: {
  extensionId: Buffer;
  versions: { version: number; wasmHash: Buffer; wasmSource: string; status: number; registeredAt: bigint }[];
  bump: number;
}): Buffer {
  const parts: Buffer[] = [];

  parts.push(anchorDiscriminator("WasmModuleAccount"));
  parts.push(opts.extensionId); // 32 bytes

  // Vec<WasmVersionEntry>
  parts.push(u32le(opts.versions.length));
  for (const v of opts.versions) {
    parts.push(u32le(v.version));
    parts.push(v.wasmHash);   // 32 bytes
    parts.push(borshString(v.wasmSource));
    parts.push(Buffer.from([v.status]));
    parts.push(u64le(v.registeredAt));
  }

  parts.push(Buffer.from([opts.bump]));
  return Buffer.concat(parts);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("chain", () => {
  describe("PDA derivation", () => {
    it("findGlobalConfigPDA returns deterministic address", () => {
      const [pda1, bump1] = findGlobalConfigPDA();
      const [pda2, bump2] = findGlobalConfigPDA();
      assert.equal(pda1.toBase58(), pda2.toBase58());
      assert.equal(bump1, bump2);
    });

    it("findGlobalConfigPDA matches manual derivation", () => {
      const [pda] = findGlobalConfigPDA(TITLE_CONFIG_PROGRAM_ID);
      const [expected] = PublicKey.findProgramAddressSync(
        [Buffer.from("global-config")],
        TITLE_CONFIG_PROGRAM_ID
      );
      assert.equal(pda.toBase58(), expected.toBase58());
    });

    it("findTeeNodePDA accepts PublicKey, string, and Uint8Array", () => {
      const pk = new PublicKey(randomBytes32());

      const [fromPk] = findTeeNodePDA(pk);
      const [fromStr] = findTeeNodePDA(pk.toBase58());
      const [fromBytes] = findTeeNodePDA(pk.toBuffer());

      assert.equal(fromPk.toBase58(), fromStr.toBase58());
      assert.equal(fromPk.toBase58(), fromBytes.toBase58());
    });

    it("findTeeNodePDA with custom programId", () => {
      const customProgram = new PublicKey(randomBytes32());
      const signingKey = randomBytes32();
      const [pda] = findTeeNodePDA(signingKey, customProgram);
      const [expected] = PublicKey.findProgramAddressSync(
        [Buffer.from("tee-node"), signingKey],
        customProgram
      );
      assert.equal(pda.toBase58(), expected.toBase58());
    });

    it("findWasmModulePDA returns deterministic address", () => {
      const [pda1] = findWasmModulePDA("image-phash");
      const [pda2] = findWasmModulePDA("image-phash");
      assert.equal(pda1.toBase58(), pda2.toBase58());
    });

    it("findWasmModulePDA matches manual derivation", () => {
      const extensionId = "image-phash";
      const [pda] = findWasmModulePDA(extensionId);
      const idBytes = Buffer.alloc(32, 0);
      Buffer.from(extensionId, "utf-8").copy(idBytes);
      const [expected] = PublicKey.findProgramAddressSync(
        [Buffer.from("wasm-module"), idBytes],
        TITLE_CONFIG_PROGRAM_ID
      );
      assert.equal(pda.toBase58(), expected.toBase58());
    });

    it("findWasmModulePDA with custom programId", () => {
      const customProgram = new PublicKey(randomBytes32());
      const [pda] = findWasmModulePDA("c2pa-training", customProgram);
      const idBytes = Buffer.alloc(32, 0);
      Buffer.from("c2pa-training", "utf-8").copy(idBytes);
      const [expected] = PublicKey.findProgramAddressSync(
        [Buffer.from("wasm-module"), idBytes],
        customProgram
      );
      assert.equal(pda.toBase58(), expected.toBase58());
    });
  });

  describe("GlobalConfigAccount deserialization", () => {
    it("deserializes empty config", () => {
      const authority = randomBytes32();
      const coreMint = randomBytes32();
      const extMint = randomBytes32();

      const buf = buildGlobalConfigBuffer({
        authority,
        coreMint,
        extMint,
        nodeKeys: [],
        tsaKeys: [],
        wasmIds: [],
      });

      const result = _deserializeGlobalConfig(buf);
      assert.equal(result.authority.toString("hex"), authority.toString("hex"));
      assert.equal(
        result.coreCollectionMint.toString("hex"),
        coreMint.toString("hex")
      );
      assert.equal(
        result.extCollectionMint.toString("hex"),
        extMint.toString("hex")
      );
      assert.equal(result.trustedNodeKeys.length, 0);
      assert.equal(result.trustedTsaKeys.length, 0);
      assert.equal(result.trustedWasmIds.length, 0);
    });

    it("deserializes config with node keys, TSA keys, and WASM IDs", () => {
      const nodeKey1 = randomBytes32();
      const nodeKey2 = randomBytes32();
      const tsaKey = randomBytes32();
      const wasmId = extensionIdBytes("image-phash");

      const buf = buildGlobalConfigBuffer({
        authority: randomBytes32(),
        coreMint: randomBytes32(),
        extMint: randomBytes32(),
        nodeKeys: [nodeKey1, nodeKey2],
        tsaKeys: [tsaKey],
        wasmIds: [wasmId],
      });

      const result = _deserializeGlobalConfig(buf);
      assert.equal(result.trustedNodeKeys.length, 2);
      assert.equal(
        result.trustedNodeKeys[0].toString("hex"),
        nodeKey1.toString("hex")
      );
      assert.equal(
        result.trustedNodeKeys[1].toString("hex"),
        nodeKey2.toString("hex")
      );
      assert.equal(result.trustedTsaKeys.length, 1);
      assert.equal(result.trustedWasmIds.length, 1);
    });

    it("deserializes resource_limits (all None)", () => {
      const buf = buildGlobalConfigBuffer({
        authority: randomBytes32(),
        coreMint: randomBytes32(),
        extMint: randomBytes32(),
        nodeKeys: [],
        tsaKeys: [],
        wasmIds: [],
      });

      const result = _deserializeGlobalConfig(buf);
      assert.equal(result.resourceLimits.max_single_content_bytes, undefined);
      assert.equal(result.resourceLimits.max_concurrent_bytes, undefined);
      assert.equal(result.resourceLimits.c2pa_max_graph_size, undefined);
    });

    it("deserializes resource_limits with values", () => {
      const limits = Buffer.concat([
        optionU64(2_000_000_000),  // max_single_content_bytes
        optionU64(8_000_000_000),  // max_concurrent_bytes
        optionU64(undefined),      // min_upload_speed_bytes
        optionU64(30),             // base_processing_time_sec
        optionU64(3600),           // max_global_timeout_sec
        optionU64(undefined),      // chunk_read_timeout_sec
        optionU64(10000),          // c2pa_max_graph_size
      ]);

      const buf = buildGlobalConfigBuffer({
        authority: randomBytes32(),
        coreMint: randomBytes32(),
        extMint: randomBytes32(),
        nodeKeys: [randomBytes32()],
        tsaKeys: [],
        wasmIds: [],
        resourceLimits: limits,
      });

      const result = _deserializeGlobalConfig(buf);
      assert.equal(result.resourceLimits.max_single_content_bytes, 2_000_000_000);
      assert.equal(result.resourceLimits.max_concurrent_bytes, 8_000_000_000);
      assert.equal(result.resourceLimits.min_upload_speed_bytes, undefined);
      assert.equal(result.resourceLimits.base_processing_time_sec, 30);
      assert.equal(result.resourceLimits.max_global_timeout_sec, 3600);
      assert.equal(result.resourceLimits.chunk_read_timeout_sec, undefined);
      assert.equal(result.resourceLimits.c2pa_max_graph_size, 10000);
    });

    it("rejects invalid discriminator", () => {
      const buf = Buffer.alloc(200);
      buf.fill(0xff, 0, 8); // wrong discriminator
      assert.throws(
        () => _deserializeGlobalConfig(buf),
        /Invalid GlobalConfig discriminator/
      );
    });
  });

  describe("TeeNodeAccount deserialization", () => {
    it("deserializes node with no measurements", () => {
      const signingPubkey = randomBytes32();
      const encryptionPubkey = randomBytes32();
      const gatewayPubkey = randomBytes32();

      const buf = buildTeeNodeBuffer({
        signingPubkey,
        encryptionPubkey,
        gatewayPubkey,
        gatewayEndpoint: "http://localhost:3000",
        status: 1,
        teeType: 0,
        measurements: [],
        bump: 254,
      });

      const result = _deserializeTeeNodeAccount(buf);
      assert.equal(
        result.signingPubkey.toString("hex"),
        signingPubkey.toString("hex")
      );
      assert.equal(
        result.encryptionPubkey.toString("hex"),
        encryptionPubkey.toString("hex")
      );
      assert.equal(result.gatewayEndpoint, "http://localhost:3000");
      assert.equal(result.status, 1);
      assert.equal(result.teeType, 0);
      assert.equal(result.measurements.length, 0);
      assert.equal(result.bump, 254);
    });

    it("deserializes node with measurements", () => {
      const mKey = measurementKey("PCR0");
      const mValue = measurementValue48();

      const buf = buildTeeNodeBuffer({
        signingPubkey: randomBytes32(),
        encryptionPubkey: randomBytes32(),
        gatewayPubkey: randomBytes32(),
        gatewayEndpoint: "https://gateway.example.com",
        status: 0,
        teeType: 1,
        measurements: [{ key: mKey, value: mValue }],
        bump: 255,
      });

      const result = _deserializeTeeNodeAccount(buf);
      assert.equal(result.teeType, 1);
      assert.equal(result.measurements.length, 1);
      assert.equal(
        result.measurements[0].key.subarray(0, 4).toString("utf-8"),
        "PCR0"
      );
      assert.equal(result.bump, 255);
    });

    it("rejects invalid discriminator", () => {
      const buf = Buffer.alloc(200);
      buf.fill(0xaa, 0, 8);
      assert.throws(
        () => _deserializeTeeNodeAccount(buf),
        /Invalid TeeNodeAccount discriminator/
      );
    });
  });

  describe("WasmModuleAccount deserialization", () => {
    it("deserializes module with no versions", () => {
      const extId = extensionIdBytes("image-phash");

      const buf = buildWasmModuleBuffer({
        extensionId: extId,
        versions: [],
        bump: 253,
      });

      const result = _deserializeWasmModuleAccount(buf);
      assert.equal(
        result.extensionId.subarray(0, 11).toString("utf-8"),
        "image-phash"
      );
      assert.equal(result.versions.length, 0);
      assert.equal(result.bump, 253);
    });

    it("deserializes module with versions", () => {
      const extId = extensionIdBytes("c2pa-training");
      const wasmHash = randomBytes32();

      const buf = buildWasmModuleBuffer({
        extensionId: extId,
        versions: [
          {
            version: 1,
            wasmHash,
            wasmSource: "https://arweave.net/abc123",
            status: 0,
            registeredAt: 1700000000n,
          },
        ],
        bump: 252,
      });

      const result = _deserializeWasmModuleAccount(buf);
      assert.equal(
        result.extensionId.subarray(0, 13).toString("utf-8"),
        "c2pa-training"
      );
      assert.equal(result.versions.length, 1);
      assert.equal(result.versions[0].version, 1);
      assert.equal(
        result.versions[0].wasmHash.toString("hex"),
        wasmHash.toString("hex")
      );
      assert.equal(result.versions[0].wasmSource, "https://arweave.net/abc123");
      assert.equal(result.versions[0].status, 0);
      assert.equal(result.versions[0].registeredAt, 1700000000n);
      assert.equal(result.bump, 252);
    });

    it("deserializes module with multiple versions", () => {
      const extId = extensionIdBytes("hardware-google");

      const buf = buildWasmModuleBuffer({
        extensionId: extId,
        versions: [
          {
            version: 1,
            wasmHash: randomBytes32(),
            wasmSource: "https://arweave.net/v1",
            status: 1, // deprecated
            registeredAt: 1700000000n,
          },
          {
            version: 2,
            wasmHash: randomBytes32(),
            wasmSource: "https://arweave.net/v2",
            status: 0, // active
            registeredAt: 1700100000n,
          },
        ],
        bump: 251,
      });

      const result = _deserializeWasmModuleAccount(buf);
      assert.equal(result.versions.length, 2);
      assert.equal(result.versions[0].version, 1);
      assert.equal(result.versions[0].status, 1);
      assert.equal(result.versions[1].version, 2);
      assert.equal(result.versions[1].status, 0);
    });

    it("rejects invalid discriminator", () => {
      const buf = Buffer.alloc(200);
      buf.fill(0xbb, 0, 8);
      assert.throws(
        () => _deserializeWasmModuleAccount(buf),
        /Invalid WasmModuleAccount discriminator/
      );
    });
  });

  describe("encoding conventions", () => {
    it("signing_pubkey and gateway_pubkey are Base58", () => {
      const testBytes = Buffer.alloc(32, 0x01);
      const b58 = bs58.encode(testBytes);
      assert.ok(b58.length > 0);
      // Roundtrip
      const decoded = bs58.decode(b58);
      assert.deepEqual(Buffer.from(decoded), testBytes);
    });

    it("encryption_pubkey is Base64", () => {
      const testBytes = Buffer.alloc(32, 0x42);
      const b64 = testBytes.toString("base64");
      assert.ok(b64.endsWith("=") || b64.length > 0);
      // Roundtrip
      const decoded = Buffer.from(b64, "base64");
      assert.deepEqual(decoded, testBytes);
    });

    it("wasm_hash and measurement values are hex", () => {
      const testBytes = Buffer.from([0xde, 0xad, 0xbe, 0xef]);
      const hex = testBytes.toString("hex");
      assert.equal(hex, "deadbeef");
    });
  });
});
