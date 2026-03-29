// SPDX-License-Identifier: Apache-2.0

/**
 * Shared test context: SDK client init, crypto polyfill, node selection.
 * Lazily initialized and cached across all test files.
 */

import { webcrypto } from "node:crypto";
if (!globalThis.crypto?.subtle) {
  // @ts-ignore
  globalThis.crypto = webcrypto;
}

import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import {
  fetchGlobalConfig,
  TitleClient,
  type GlobalConfig,
} from "@title-protocol/sdk";
import type { TeeSession } from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// Config (env-overridable)
// ---------------------------------------------------------------------------

export const GATEWAY_URL =
  process.env.TEST_GATEWAY_URL ?? "http://localhost:3000";
export const SOLANA_RPC =
  process.env.TEST_SOLANA_RPC ?? "https://api.devnet.solana.com";
export const PROGRAM_ID =
  process.env.TEST_PROGRAM_ID ?? "HLdrA1s96z9rTsWMP9H8HrZXcV4guFkCyFrdKGBdAMyC";

// Deterministic test wallet (not used for signing, just an address)
const TEST_KEYPAIR = Keypair.generate();
export const OWNER_WALLET = TEST_KEYPAIR.publicKey.toBase58();

// ---------------------------------------------------------------------------
// Cached context
// ---------------------------------------------------------------------------

export interface TestContext {
  config: GlobalConfig;
  client: TitleClient;
  session: TeeSession;
}

let cached: TestContext | null = null;

export async function getTestContext(): Promise<TestContext> {
  if (cached) return cached;

  const connection = new Connection(SOLANA_RPC);
  const config = await fetchGlobalConfig(
    connection,
    new PublicKey(PROGRAM_ID)
  );
  const client = new TitleClient(config);
  const session = await client.selectNodeByEndpoint(GATEWAY_URL);

  cached = { config, client, session };
  return cached;
}
