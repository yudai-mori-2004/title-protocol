// SPDX-License-Identifier: Apache-2.0

/**
 * Shared helpers for performance tests.
 * Reuses integration test setup for SDK initialization.
 */

import { webcrypto } from "node:crypto";
if (!globalThis.crypto?.subtle) {
  // @ts-ignore
  globalThis.crypto = webcrypto;
}

import * as fs from "node:fs";
import * as path from "node:path";
import { Connection, PublicKey } from "@solana/web3.js";
import {
  TitleClient,
  fetchGlobalConfig,
  encryptPayload,
  buildPlaintext,
  decryptResponse,
  type TeeSession,
} from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

export const GATEWAY_URL = process.env.TEST_GATEWAY_URL ?? "http://localhost:3000";
export const SOLANA_RPC = process.env.TEST_SOLANA_RPC ?? "https://api.devnet.solana.com";
export const PROGRAM_ID = process.env.TEST_PROGRAM_ID ?? "HLdrA1s96z9rTsWMP9H8HrZXcV4guFkCyFrdKGBdAMyC";
const WALLET_ADDR = "TestPerfWallet11111111111111111111111111111";

const ROOT = path.resolve(import.meta.dirname, "../..");

// ---------------------------------------------------------------------------
// Lazy init
// ---------------------------------------------------------------------------

let client: TitleClient;
let session: TeeSession;
let initialized = false;

export async function init(): Promise<void> {
  if (initialized) return;
  const conn = new Connection(SOLANA_RPC);
  const config = await fetchGlobalConfig(conn, new PublicKey(PROGRAM_ID));
  client = new TitleClient(config);
  session = await client.selectNodeByEndpoint(GATEWAY_URL);
  initialized = true;
}

export function getClient(): TitleClient { return client; }
export function getSession(): TeeSession { return session; }

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

export function loadFixture(relativePath: string): Uint8Array {
  return new Uint8Array(fs.readFileSync(path.join(ROOT, relativePath)));
}

/** Encrypt + upload, return downloadUrl + symmetric key. */
export async function uploadEncrypted(content: Uint8Array): Promise<{
  downloadUrl: string;
  symmetricKey: Uint8Array;
}> {
  const plaintext = buildPlaintext({ owner_wallet: WALLET_ADDR }, content);
  const { symmetricKey, payload } = await encryptPayload(
    Buffer.from(session.encryptionPubkey, "base64"),
    plaintext,
  );
  const { downloadUrl } = await client.upload(session.gatewayUrl, payload);
  return { downloadUrl, symmetricKey };
}

/** Full verify cycle: encrypt → upload → verify → decrypt. Returns duration in ms. */
export async function timedVerify(
  content: Uint8Array,
  processors: string[] = ["core-c2pa"],
): Promise<{ durationMs: number; results: any[] }> {
  const { downloadUrl, symmetricKey } = await uploadEncrypted(content);

  const t0 = Date.now();
  const encResp = await client.verifyRaw(session.gatewayUrl, {
    download_url: downloadUrl,
    processor_ids: processors,
  });
  const durationMs = Date.now() - t0;

  const plain = await decryptResponse(symmetricKey, encResp.nonce, encResp.ciphertext);
  const parsed = JSON.parse(new TextDecoder().decode(plain));
  return { durationMs, results: parsed.results };
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

export interface Stats {
  count: number;
  successCount: number;
  min: number;
  max: number;
  avg: number;
  p50: number;
  p95: number;
  p99: number;
}

export function calcStats(durations: number[]): Stats {
  const sorted = [...durations].sort((a, b) => a - b);
  const n = sorted.length;
  return {
    count: n,
    successCount: n,
    min: sorted[0] ?? 0,
    max: sorted[n - 1] ?? 0,
    avg: n > 0 ? Math.round(sorted.reduce((a, b) => a + b, 0) / n) : 0,
    p50: sorted[Math.floor(n * 0.5)] ?? 0,
    p95: sorted[Math.floor(n * 0.95)] ?? 0,
    p99: sorted[Math.floor(n * 0.99)] ?? 0,
  };
}

export function formatStats(label: string, s: Stats): string {
  return `${label}: avg=${s.avg}ms p50=${s.p50}ms p95=${s.p95}ms p99=${s.p99}ms min=${s.min}ms max=${s.max}ms (n=${s.count})`;
}
