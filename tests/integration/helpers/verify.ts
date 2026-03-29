// SPDX-License-Identifier: Apache-2.0

/**
 * Core test helper: encrypt -> upload -> verify -> decrypt.
 * Every test calls this instead of reimplementing the cycle.
 */

import * as fs from "node:fs";
import {
  buildPlaintext,
  encryptPayload,
  decryptResponse,
} from "@title-protocol/sdk";
import type { VerifyResponse, SignedJson } from "@title-protocol/sdk";
import { getTestContext, OWNER_WALLET } from "./setup.ts";
import { fixturePath } from "./fixtures.ts";

export interface VerifyResult {
  response: VerifyResponse;
  results: VerifyResponse["results"];
  core: SignedJson | undefined;
  extensions: Map<string, SignedJson>;
  durationMs: number;
}

export async function verifyContent(
  relativeFixturePath: string,
  processorIds: string[],
): Promise<VerifyResult> {
  const ctx = await getTestContext();
  const absPath = fixturePath(relativeFixturePath);
  const content = fs.readFileSync(absPath);

  const plaintext = buildPlaintext(
    { owner_wallet: OWNER_WALLET },
    new Uint8Array(content),
  );
  const teeEncPubkey = Buffer.from(ctx.session.encryptionPubkey, "base64");
  const { symmetricKey, payload } = await encryptPayload(
    new Uint8Array(teeEncPubkey),
    plaintext,
  );

  const { downloadUrl } = await ctx.client.upload(
    ctx.session.gatewayUrl,
    payload,
  );

  const t0 = Date.now();
  const encResponse = await ctx.client.verifyRaw(ctx.session.gatewayUrl, {
    download_url: downloadUrl,
    processor_ids: processorIds,
  });
  const durationMs = Date.now() - t0;

  const responsePlaintext = await decryptResponse(
    symmetricKey,
    encResponse.nonce,
    encResponse.ciphertext,
  );
  const response: VerifyResponse = JSON.parse(
    new TextDecoder().decode(responsePlaintext),
  );

  let core: SignedJson | undefined;
  const extensions = new Map<string, SignedJson>();

  for (const r of response.results) {
    if (r.processor_id === "core-c2pa") {
      core = r.signed_json;
    } else {
      const p = r.signed_json.payload as any;
      extensions.set(p.extension_id ?? r.processor_id, r.signed_json);
    }
  }

  return { response, results: response.results, core, extensions, durationMs };
}
