// SPDX-License-Identifier: Apache-2.0

/**
 * On-chain PDA read helpers for Title Protocol.
 *
 * Reads GlobalConfigAccount and TeeNodeAccount PDAs from the Solana
 * title-config program, deserializes Anchor/Borsh data, and returns
 * SDK-level types (GlobalConfig, TrustedTeeNode, etc.).
 *
 * Spec §5.2 Step 1, §8
 */

import { Connection, PublicKey } from "@solana/web3.js";
import bs58 from "bs58";
import type {
  GlobalConfig,
  ResourceLimits,
  TrustedTeeNode,
  WasmModuleInfo,
  WasmVersionInfo,
  ExpectedMeasurements,
} from "./types";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Supported cluster names. */
export type TitleCluster = "devnet" | "mainnet";

/** Known Title Config program IDs per cluster. */
export const TITLE_CONFIG_PROGRAM_IDS: Record<TitleCluster, PublicKey | null> = {
  devnet: new PublicKey("HLdrA1s96z9rTsWMP9H8HrZXcV4guFkCyFrdKGBdAMyC"),
  mainnet: null, // TBD — DAO deployment
};

/** Default RPC URLs per cluster. */
export const DEFAULT_RPC_URLS: Record<TitleCluster, string> = {
  devnet: "https://api.devnet.solana.com",
  mainnet: "https://api.mainnet-beta.solana.com",
};

/** Default program ID (devnet). */
export const TITLE_CONFIG_PROGRAM_ID = TITLE_CONFIG_PROGRAM_IDS.devnet!;

/**
 * Resolve program ID for a cluster.
 * Throws if the cluster's program is not yet deployed.
 */
export function getProgramId(cluster: TitleCluster): PublicKey {
  const id = TITLE_CONFIG_PROGRAM_IDS[cluster];
  if (!id) {
    throw new Error(`Title Protocol is not yet deployed on ${cluster}`);
  }
  return id;
}

/** Anchor account discriminator for GlobalConfigAccount. */
const GLOBAL_CONFIG_DISC = Buffer.from("58c97d0fc786e147", "hex");

/** Anchor account discriminator for TeeNodeAccount. */
const TEE_NODE_DISC = Buffer.from("a3bc3b8a54edb493", "hex");

/** Anchor account discriminator for WasmModuleAccount. */
const WASM_MODULE_DISC = Buffer.from("a2fe81fbbda3bfb0", "hex");

// ---------------------------------------------------------------------------
// Status / TeeType enums
// ---------------------------------------------------------------------------

const STATUS_MAP: Record<number, string> = {
  0: "inactive",
  1: "active",
};

const TEE_TYPE_MAP: Record<number, string> = {
  0: "aws_nitro",
  1: "amd_sev_snp",
  2: "intel_tdx",
};

/** On-chain signing algorithm u8 → string. */
const SIGNING_ALGORITHM_MAP: Record<number, string> = {
  0: "ed25519",
};

/** On-chain encryption algorithm u8 → string. Matches Rust KemAlgorithm::as_str(). */
const ENCRYPTION_ALGORITHM_MAP: Record<number, string> = {
  0: "x25519-hkdf-sha256",
};

// ---------------------------------------------------------------------------
// PDA derivation
// ---------------------------------------------------------------------------

/**
 * Derive the GlobalConfig PDA address.
 *
 * 仕様書 §8 — seeds = ["global-config"]
 */
export function findGlobalConfigPDA(
  programId: PublicKey = TITLE_CONFIG_PROGRAM_ID
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("global-config")],
    programId
  );
}

/**
 * Derive a TeeNodeAccount PDA address.
 *
 * 仕様書 §8 — seeds = ["tee-node", signing_pubkey]
 *
 * @param signingPubkey - Ed25519 signing pubkey (PublicKey, Base58 string, or 32-byte Buffer)
 */
export function findTeeNodePDA(
  signingPubkey: PublicKey | string | Uint8Array,
  programId: PublicKey = TITLE_CONFIG_PROGRAM_ID
): [PublicKey, number] {
  let bytes: Uint8Array;
  if (signingPubkey instanceof PublicKey) {
    bytes = signingPubkey.toBuffer();
  } else if (typeof signingPubkey === "string") {
    bytes = new PublicKey(signingPubkey).toBuffer();
  } else {
    bytes = signingPubkey;
  }
  return PublicKey.findProgramAddressSync(
    [Buffer.from("tee-node"), bytes],
    programId
  );
}

/**
 * Derive a WasmModuleAccount PDA address.
 *
 * 仕様書 §7.3 — seeds = ["wasm-module", extension_id]
 *
 * @param extensionId - Extension ID string (e.g. "phash-v1"). Will be null-padded to 32 bytes.
 */
export function findWasmModulePDA(
  extensionId: string,
  programId: PublicKey = TITLE_CONFIG_PROGRAM_ID
): [PublicKey, number] {
  const idBytes = Buffer.alloc(32, 0);
  Buffer.from(extensionId, "utf-8").copy(idBytes, 0, 0, Math.min(extensionId.length, 32));
  return PublicKey.findProgramAddressSync(
    [Buffer.from("wasm-module"), idBytes],
    programId
  );
}

// ---------------------------------------------------------------------------
// Borsh deserialization helpers
// ---------------------------------------------------------------------------

/** Minimal cursor-based reader for Borsh-encoded account data. */
class BorshReader {
  private offset: number;
  constructor(private readonly buf: Buffer) {
    this.offset = 0;
  }

  /** Current read position */
  pos(): number {
    return this.offset;
  }

  readBytes(n: number): Buffer {
    if (this.offset + n > this.buf.length) {
      throw new Error(
        `BorshReader: buffer overrun (offset=${this.offset}, need=${n}, have=${this.buf.length - this.offset})`
      );
    }
    const slice = this.buf.subarray(this.offset, this.offset + n);
    this.offset += n;
    return slice;
  }

  readU8(): number {
    const v = this.buf.readUInt8(this.offset);
    this.offset += 1;
    return v;
  }

  readU32LE(): number {
    const v = this.buf.readUInt32LE(this.offset);
    this.offset += 4;
    return v;
  }

  readFixedBytes(n: number): Buffer {
    return this.readBytes(n);
  }

  readPubkey(): Buffer {
    return this.readFixedBytes(32);
  }

  readU64LE(): bigint {
    const low = this.buf.readUInt32LE(this.offset);
    const high = this.buf.readUInt32LE(this.offset + 4);
    this.offset += 8;
    return BigInt(low) + (BigInt(high) << 32n);
  }

  readOptionU64(): number | undefined {
    const tag = this.readU8();
    if (tag === 0) return undefined;
    return Number(this.readU64LE());
  }

  readString(): string {
    const len = this.readU32LE();
    const bytes = this.readBytes(len);
    return bytes.toString("utf-8");
  }

  /** Read a Borsh Vec<u8>: 4-byte LE length + raw bytes. */
  readVec(): Buffer {
    const len = this.readU32LE();
    return this.readBytes(len);
  }
}

// ---------------------------------------------------------------------------
// Account deserialization
// ---------------------------------------------------------------------------

interface RawGlobalConfig {
  authority: Buffer;
  coreCollectionMint: Buffer;
  extCollectionMint: Buffer;
  trustedNodeKeys: Buffer[];
  trustedTsaKeys: Buffer[];
  trustedWasmIds: Buffer[];
  resourceLimits: ResourceLimits;
}

interface RawTeeNodeAccount {
  solanaPubkey: Buffer;
  protocolSigningAlgorithm: number;
  protocolSigningPubkey: Buffer;
  encryptionAlgorithm: number;
  encryptionPubkey: Buffer;
  gatewaySigningAlgorithm: number;
  gatewayPubkey: Buffer;
  gatewayEndpoint: string;
  status: number;
  teeType: number;
  measurements: RawMeasurementEntry[];
  bump: number;
}

interface RawMeasurementEntry {
  key: Buffer;  // 16 bytes
  value: Buffer; // 48 bytes
}

function deserializeGlobalConfig(data: Buffer): RawGlobalConfig {
  const r = new BorshReader(data);

  // 8-byte Anchor discriminator
  const disc = r.readBytes(8);
  if (!disc.equals(GLOBAL_CONFIG_DISC)) {
    throw new Error(
      `Invalid GlobalConfig discriminator: ${disc.toString("hex")} (expected ${GLOBAL_CONFIG_DISC.toString("hex")})`
    );
  }

  const authority = r.readPubkey();
  const coreCollectionMint = r.readPubkey();
  const extCollectionMint = r.readPubkey();

  // Vec<[u8; 32]> trusted_node_keys
  const nodeKeysLen = r.readU32LE();
  const trustedNodeKeys: Buffer[] = [];
  for (let i = 0; i < nodeKeysLen; i++) {
    trustedNodeKeys.push(r.readFixedBytes(32));
  }

  // Vec<[u8; 32]> trusted_tsa_keys
  const tsaKeysLen = r.readU32LE();
  const trustedTsaKeys: Buffer[] = [];
  for (let i = 0; i < tsaKeysLen; i++) {
    trustedTsaKeys.push(r.readFixedBytes(32));
  }

  // Vec<[u8; 32]> trusted_wasm_ids
  const wasmIdsLen = r.readU32LE();
  const trustedWasmIds: Buffer[] = [];
  for (let i = 0; i < wasmIdsLen; i++) {
    trustedWasmIds.push(r.readFixedBytes(32));
  }

  // ResourceLimitsOnChain: 7 × Option<u64>
  const resourceLimits: ResourceLimits = {
    max_single_content_bytes: r.readOptionU64(),
    max_concurrent_bytes: r.readOptionU64(),
    min_upload_speed_bytes: r.readOptionU64(),
    base_processing_time_sec: r.readOptionU64(),
    max_global_timeout_sec: r.readOptionU64(),
    chunk_read_timeout_sec: r.readOptionU64(),
    c2pa_max_graph_size: r.readOptionU64(),
  };

  return {
    authority,
    coreCollectionMint,
    extCollectionMint,
    trustedNodeKeys,
    trustedTsaKeys,
    trustedWasmIds,
    resourceLimits,
  };
}

function deserializeTeeNodeAccount(data: Buffer): RawTeeNodeAccount {
  const r = new BorshReader(data);

  // 8-byte Anchor discriminator
  const disc = r.readBytes(8);
  if (!disc.equals(TEE_NODE_DISC)) {
    throw new Error(
      `Invalid TeeNodeAccount discriminator: ${disc.toString("hex")} (expected ${TEE_NODE_DISC.toString("hex")})`
    );
  }

  const solanaPubkey = r.readPubkey();
  const protocolSigningAlgorithm = r.readU8();
  const protocolSigningPubkey = r.readVec();
  const encryptionAlgorithm = r.readU8();
  const encryptionPubkey = r.readVec();
  const gatewaySigningAlgorithm = r.readU8();
  const gatewayPubkey = r.readVec();
  const gatewayEndpoint = r.readString();
  const status = r.readU8();
  const teeType = r.readU8();

  // Vec<MeasurementEntry>
  const measLen = r.readU32LE();
  const measurements: RawMeasurementEntry[] = [];
  for (let i = 0; i < measLen; i++) {
    const key = r.readFixedBytes(16);
    const value = r.readFixedBytes(48);
    measurements.push({ key, value });
  }

  const bump = r.readU8();

  return {
    solanaPubkey,
    protocolSigningAlgorithm,
    protocolSigningPubkey,
    encryptionAlgorithm,
    encryptionPubkey,
    gatewaySigningAlgorithm,
    gatewayPubkey,
    gatewayEndpoint,
    status,
    teeType,
    measurements,
    bump,
  };
}

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

function pubkeyToBase58(buf: Buffer): string {
  return bs58.encode(buf);
}

function bytesToBase64(buf: Buffer): string {
  return Buffer.from(buf).toString("base64");
}

function bytesToHex(buf: Buffer): string {
  return Buffer.from(buf).toString("hex");
}

/** Trim null bytes from a fixed-size byte array to extract the string content. */
function trimNulls(buf: Buffer): string {
  const end = buf.indexOf(0);
  return buf.subarray(0, end === -1 ? buf.length : end).toString("utf-8");
}

function rawToTrustedTeeNode(raw: RawTeeNodeAccount): TrustedTeeNode {
  const expectedMeasurements: ExpectedMeasurements = {};
  for (const m of raw.measurements) {
    const keyStr = trimNulls(m.key);
    expectedMeasurements[keyStr] = bytesToHex(m.value);
  }

  return {
    solana_pubkey: pubkeyToBase58(raw.solanaPubkey),
    signing_algorithm: SIGNING_ALGORITHM_MAP[raw.protocolSigningAlgorithm] ?? `unknown(${raw.protocolSigningAlgorithm})`,
    signing_pubkey: bytesToBase64(raw.protocolSigningPubkey),
    encryption_algorithm: ENCRYPTION_ALGORITHM_MAP[raw.encryptionAlgorithm] ?? `unknown(${raw.encryptionAlgorithm})`,
    encryption_pubkey: bytesToBase64(raw.encryptionPubkey),
    gateway_signing_algorithm: SIGNING_ALGORITHM_MAP[raw.gatewaySigningAlgorithm] ?? `unknown(${raw.gatewaySigningAlgorithm})`,
    gateway_pubkey: bytesToBase64(raw.gatewayPubkey),
    gateway_endpoint: raw.gatewayEndpoint,
    status: STATUS_MAP[raw.status] ?? `unknown(${raw.status})`,
    tee_type: TEE_TYPE_MAP[raw.teeType] ?? `unknown(${raw.teeType})`,
    expected_measurements: expectedMeasurements,
  };
}

function rawWasmIdToString(buf: Buffer): string {
  return trimNulls(buf);
}

// ---------------------------------------------------------------------------
// WasmModuleAccount deserialization
// ---------------------------------------------------------------------------

interface RawWasmVersionEntry {
  version: number;
  wasmHash: Buffer;  // 32 bytes
  wasmSource: string;
  status: number;
  registeredAt: bigint;
}

interface RawWasmModuleAccount {
  extensionId: Buffer;  // 32 bytes
  versions: RawWasmVersionEntry[];
  bump: number;
}

function deserializeWasmModuleAccount(data: Buffer): RawWasmModuleAccount {
  const r = new BorshReader(data);

  const disc = r.readBytes(8);
  if (!disc.equals(WASM_MODULE_DISC)) {
    throw new Error(
      `Invalid WasmModuleAccount discriminator: ${disc.toString("hex")} (expected ${WASM_MODULE_DISC.toString("hex")})`
    );
  }

  const extensionId = r.readFixedBytes(32);

  const versionsLen = r.readU32LE();
  const versions: RawWasmVersionEntry[] = [];
  for (let i = 0; i < versionsLen; i++) {
    const version = r.readU32LE();
    const wasmHash = r.readFixedBytes(32);
    const wasmSource = r.readString();
    const status = r.readU8();
    const registeredAt = r.readU64LE();
    versions.push({ version, wasmHash, wasmSource, status, registeredAt });
  }

  const bump = r.readU8();

  return { extensionId, versions, bump };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Fetch a single TeeNodeAccount from chain.
 *
 * @returns TrustedTeeNode or null if not found.
 */
export async function fetchTeeNodeAccount(
  connection: Connection,
  signingPubkey: PublicKey | string | Uint8Array,
  programId: PublicKey = TITLE_CONFIG_PROGRAM_ID
): Promise<TrustedTeeNode | null> {
  const [pda] = findTeeNodePDA(signingPubkey, programId);
  const accountInfo = await connection.getAccountInfo(pda);
  if (!accountInfo) return null;

  const raw = deserializeTeeNodeAccount(Buffer.from(accountInfo.data));
  return rawToTrustedTeeNode(raw);
}

/**
 * Fetch GlobalConfig and all TeeNodeAccount / WasmModuleAccount PDAs in parallel.
 *
 * Reads GlobalConfigAccount to get authority, collections, trusted keys,
 * and WASM modules. Then fetches each TeeNodeAccount and WasmModuleAccount
 * PDA and assembles a complete GlobalConfig.
 *
 * The default `programId` points to the canonical Title Protocol program.
 * The canonical GlobalConfig (controlled by the DAO multi-sig on mainnet)
 * designates the official cNFT collections — only content registered
 * through this GlobalConfig is recognized as protocol-canonical.
 * Anyone can deploy their own program and GlobalConfig, but verifiers
 * only check the canonical one.
 *
 * 仕様書 §5.2 Step 1
 */
/** Fetch GlobalConfig using default RPC for the given cluster. */
export async function fetchGlobalConfig(
  cluster: TitleCluster
): Promise<GlobalConfig>;

/** Fetch GlobalConfig with a custom RPC Connection + cluster. */
export async function fetchGlobalConfig(
  connection: Connection,
  cluster: TitleCluster
): Promise<GlobalConfig>;

/** Fetch GlobalConfig with a custom RPC Connection + custom program ID (node operators only). */
export async function fetchGlobalConfig(
  connection: Connection,
  programId: PublicKey
): Promise<GlobalConfig>;

export async function fetchGlobalConfig(
  connectionOrCluster: Connection | TitleCluster,
  clusterOrProgramId?: TitleCluster | PublicKey
): Promise<GlobalConfig> {
  let connection: Connection;
  let resolvedProgramId: PublicKey;

  if (typeof connectionOrCluster === "string") {
    // fetchGlobalConfig("devnet")
    const cluster = connectionOrCluster;
    resolvedProgramId = getProgramId(cluster);
    connection = new Connection(DEFAULT_RPC_URLS[cluster]);
  } else if (typeof clusterOrProgramId === "string") {
    // fetchGlobalConfig(connection, "devnet")
    connection = connectionOrCluster;
    resolvedProgramId = getProgramId(clusterOrProgramId);
  } else {
    // fetchGlobalConfig(connection, programId)
    connection = connectionOrCluster;
    resolvedProgramId = clusterOrProgramId ?? TITLE_CONFIG_PROGRAM_ID;
  }
  const [pda] = findGlobalConfigPDA(resolvedProgramId);
  const accountInfo = await connection.getAccountInfo(pda);
  if (!accountInfo) {
    throw new Error(
      `GlobalConfig account not found at ${pda.toBase58()}. ` +
        `Has the program been initialized?`
    );
  }

  const raw = deserializeGlobalConfig(Buffer.from(accountInfo.data));

  // Fetch all TeeNodeAccount PDAs in parallel
  const nodePromises = raw.trustedNodeKeys.map((keyBuf) =>
    fetchTeeNodeAccount(connection, keyBuf, resolvedProgramId)
  );
  const nodeResults = await Promise.all(nodePromises);
  const trustedTeeNodes = nodeResults.filter(
    (n): n is TrustedTeeNode => n !== null
  );

  // Fetch all WasmModuleAccount PDAs in parallel
  const wasmIds = raw.trustedWasmIds.map(rawWasmIdToString);
  const wasmPromises = wasmIds.map((id) =>
    fetchWasmModuleAccount(connection, id, resolvedProgramId)
  );
  const wasmResults = await Promise.all(wasmPromises);
  const trustedWasmModules = wasmResults.filter(
    (w): w is WasmModuleInfo => w !== null
  );

  return {
    authority: pubkeyToBase58(raw.authority),
    core_collection_mint: pubkeyToBase58(raw.coreCollectionMint),
    ext_collection_mint: pubkeyToBase58(raw.extCollectionMint),
    trusted_tee_nodes: trustedTeeNodes,
    trusted_tsa_keys: raw.trustedTsaKeys.map(pubkeyToBase58),
    trusted_wasm_modules: trustedWasmModules,
    resource_limits: raw.resourceLimits,
  };
}

/**
 * Fetch a single WasmModuleAccount from chain.
 *
 * @returns WasmModuleInfo or null if not found.
 */
export async function fetchWasmModuleAccount(
  connection: Connection,
  extensionId: string,
  programId: PublicKey = TITLE_CONFIG_PROGRAM_ID
): Promise<WasmModuleInfo | null> {
  const [pda] = findWasmModulePDA(extensionId, programId);
  const accountInfo = await connection.getAccountInfo(pda);
  if (!accountInfo) return null;

  const raw = deserializeWasmModuleAccount(Buffer.from(accountInfo.data));
  return {
    extension_id: trimNulls(raw.extensionId),
    versions: raw.versions.map((v) => ({
      version: v.version,
      wasm_hash: bytesToHex(v.wasmHash),
      wasm_source: v.wasmSource,
      status: v.status,
      registered_at: Number(v.registeredAt),
    })),
  };
}

// Re-export for deserialization testing
export { deserializeGlobalConfig as _deserializeGlobalConfig };
export { deserializeTeeNodeAccount as _deserializeTeeNodeAccount };
export { deserializeWasmModuleAccount as _deserializeWasmModuleAccount };
