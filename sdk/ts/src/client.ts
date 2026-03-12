// SPDX-License-Identifier: Apache-2.0

/**
 * TitleClient — Main SDK class.
 *
 * Spec §6.7
 */

import type {
  GlobalConfig,
  TrustedTeeNode,
  TrustedWasmModule,
  VerifyRequest,
  VerifyResponse,
  SignRequest,
  SignResponse,
  EncryptedPayload,
  ExtensionPayload,
} from "./types";
import { encryptPayload, decryptResponse } from "./crypto";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Session with a specific TEE node. */
export interface TeeSession {
  gatewayUrl: string;
  encryptionPubkey: string;
  signingPubkey: string;
}

/**
 * Callback to persist a signed_json to permanent storage.
 * Receives the JSON string, returns a retrievable URI (e.g. ar://...).
 */
export type StoreSignedJsonFn = (json: string) => Promise<string>;

/** Options for `client.register()`. */
export interface RegisterOptions {
  /** Content binary data. */
  content: Uint8Array;
  /** Owner wallet address (Base58). */
  ownerWallet: string;
  /** Processor IDs to execute. Default: `["core-c2pa"]`. */
  processorIds?: string[];
  /** Optional extension auxiliary inputs. Key = extension_id. */
  extensionInputs?: Record<string, unknown>;
  /** Callback to persist each signed_json. Returns a URI. */
  storeSignedJson: StoreSignedJsonFn;
  /** Use a specific node instead of auto-selecting. */
  node?: TeeSession;
  /** If true, Gateway broadcasts the TX. Default: false. */
  delegateMint?: boolean;
  /** Solana recent blockhash (required when delegateMint is false). */
  recentBlockhash?: string;
}

/** Single content entry in the register result. */
export interface RegisterContentResult {
  processorId: string;
  contentHash: string;
  storageUri: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  signedJson: any;
}

/** Result of `client.register()`. */
export interface RegisterResult {
  contents: RegisterContentResult[];
  /** Base64-encoded partial TXs (delegateMint: false). */
  partialTxs?: string[];
  /** On-chain TX signatures (delegateMint: true). */
  txSignatures?: string[];
}

// ---------------------------------------------------------------------------
// TitleClient
// ---------------------------------------------------------------------------

export class TitleClient {
  readonly globalConfig: GlobalConfig;
  private availableNodes: TrustedTeeNode[];

  constructor(globalConfig: GlobalConfig) {
    this.globalConfig = globalConfig;
    this.availableNodes = [...globalConfig.trusted_tee_nodes];
  }

  /**
   * Select a live TEE node via health-check.
   * Deduplicates by gateway_endpoint (keeps newest entry).
   */
  async selectNode(): Promise<TeeSession> {
    const byEndpoint = new Map<string, TrustedTeeNode>();
    for (const node of this.availableNodes) {
      const ep = node.gateway_endpoint.replace(/\/$/, "");
      byEndpoint.set(ep, node);
    }

    const candidates = [...byEndpoint.values()];
    while (candidates.length > 0) {
      const idx = Math.floor(Math.random() * candidates.length);
      const node = candidates[idx];
      const base = node.gateway_endpoint.replace(/\/$/, "");

      try {
        const res = await fetch(`${base}/health`);
        if (res.status === 404) {
          candidates.splice(idx, 1);
          continue;
        }
      } catch {
        candidates.splice(idx, 1);
        continue;
      }

      return {
        gatewayUrl: base,
        encryptionPubkey: node.encryption_pubkey,
        signingPubkey: node.signing_pubkey,
      };
    }

    throw new Error("No healthy TEE node found");
  }

  /**
   * Register content: encrypt → upload → verify → store → sign.
   * Spec §6.7
   */
  async register(options: RegisterOptions): Promise<RegisterResult> {
    const {
      content,
      ownerWallet,
      processorIds = ["core-c2pa"],
      extensionInputs,
      storeSignedJson,
      delegateMint = false,
      recentBlockhash,
    } = options;

    // 1. Node selection
    const node = options.node ?? (await this.selectNode());

    // 2-3. Encrypt + upload
    const contentB64 = Buffer.from(content).toString("base64");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const clientPayload: any = {
      owner_wallet: ownerWallet,
      content: contentB64,
    };
    if (extensionInputs) {
      clientPayload.extension_inputs = extensionInputs;
    }

    const payloadJson = new TextEncoder().encode(
      JSON.stringify(clientPayload)
    );
    const teeEncPubkey = Buffer.from(node.encryptionPubkey, "base64");
    const { symmetricKey, encryptedPayload } = await encryptPayload(
      new Uint8Array(teeEncPubkey),
      payloadJson
    );

    const { downloadUrl } = await this.upload(
      node.gatewayUrl,
      encryptedPayload
    );

    // 4-5. Verify + decrypt
    const encryptedResponse = await this.verifyRaw(node.gatewayUrl, {
      download_url: downloadUrl,
      processor_ids: processorIds,
    });

    const responsePlaintext = await decryptResponse(
      symmetricKey,
      encryptedResponse.nonce,
      encryptedResponse.ciphertext
    );
    const verifyResponse: VerifyResponse = JSON.parse(
      new TextDecoder().decode(responsePlaintext)
    );

    // 6. wasm_hash validation
    this.validateWasmHashes(verifyResponse);

    // 7. Store signed_json via callback
    const contents: RegisterContentResult[] = [];
    const signRequests: { signed_json_uri: string }[] = [];

    for (const result of verifyResponse.results) {
      const sj = result.signed_json;
      const jsonStr = JSON.stringify(sj);
      const uri = await storeSignedJson(jsonStr);

      const payload = sj.payload as { content_hash?: string };
      contents.push({
        processorId: result.processor_id,
        contentHash: payload.content_hash ?? "",
        storageUri: uri,
        signedJson: sj,
      });
      signRequests.push({ signed_json_uri: uri });
    }

    // 8. Sign or sign-and-mint
    if (delegateMint) {
      const res = await this.signAndMintRaw(node.gatewayUrl, {
        recent_blockhash: recentBlockhash ?? "",
        requests: signRequests,
      });
      return { contents, txSignatures: res.tx_signatures };
    } else {
      if (!recentBlockhash) {
        throw new Error(
          "recentBlockhash is required when delegateMint is false"
        );
      }
      const res = await this.signRaw(node.gatewayUrl, {
        recent_blockhash: recentBlockhash,
        requests: signRequests,
      });
      return { contents, partialTxs: res.partial_txs };
    }
  }

  // ---------------------------------------------------------------------------
  // Low-level Gateway methods (kept public for advanced use)
  // ---------------------------------------------------------------------------

  /** Get a presigned upload URL. */
  async getUploadUrl(
    gatewayUrl: string,
    contentSize: number,
    contentType: string
  ): Promise<{ uploadUrl: string; downloadUrl: string; expiresAt: number }> {
    const res = await this.gatewayPost(gatewayUrl, "/upload-url", {
      content_size: contentSize,
      content_type: contentType,
    });
    return {
      uploadUrl: res.upload_url,
      downloadUrl: res.download_url,
      expiresAt: res.expires_at,
    };
  }

  /** Upload an encrypted payload to temporary storage. */
  async upload(
    gatewayUrl: string,
    encryptedPayload: EncryptedPayload
  ): Promise<{ downloadUrl: string; sizeBytes: number }> {
    const payloadBytes = new TextEncoder().encode(
      JSON.stringify(encryptedPayload)
    );

    const { uploadUrl, downloadUrl } = await this.getUploadUrl(
      gatewayUrl,
      payloadBytes.length,
      "application/json"
    );

    const putRes = await fetch(uploadUrl, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: payloadBytes,
    });
    if (!putRes.ok) {
      throw new Error(
        `Failed to upload to temporary storage: HTTP ${putRes.status}`
      );
    }

    return { downloadUrl, sizeBytes: payloadBytes.length };
  }

  /** Call /verify (returns encrypted response). */
  async verifyRaw(
    gatewayUrl: string,
    request: VerifyRequest
  ): Promise<{ nonce: string; ciphertext: string }> {
    const res = await this.gatewayPost(gatewayUrl, "/verify", request);
    return { nonce: res.nonce, ciphertext: res.ciphertext };
  }

  /** Call /sign. */
  async signRaw(
    gatewayUrl: string,
    request: SignRequest
  ): Promise<SignResponse> {
    return await this.gatewayPost(gatewayUrl, "/sign", request);
  }

  /** Call /sign-and-mint. */
  async signAndMintRaw(
    gatewayUrl: string,
    request: SignRequest
  ): Promise<{ tx_signatures: string[] }> {
    return await this.gatewayPost(gatewayUrl, "/sign-and-mint", request);
  }

  // ---------------------------------------------------------------------------
  // GlobalConfig accessors
  // ---------------------------------------------------------------------------

  getTrustedWasmModules(): TrustedWasmModule[] {
    return this.globalConfig.trusted_wasm_modules;
  }

  getCoreCollectionMint(): string {
    return this.globalConfig.core_collection_mint;
  }

  getExtCollectionMint(): string {
    return this.globalConfig.ext_collection_mint;
  }

  getTrustedTeeNodes(): TrustedTeeNode[] {
    return this.globalConfig.trusted_tee_nodes;
  }

  // ---------------------------------------------------------------------------
  // Validation
  // ---------------------------------------------------------------------------

  /**
   * Validate wasm_hash in extension signed_json against GlobalConfig.
   * Throws if any extension's wasm_hash is not in trusted_wasm_modules.
   */
  private validateWasmHashes(response: VerifyResponse): void {
    const trustedHashes = new Set(
      this.globalConfig.trusted_wasm_modules.map((m) => m.wasm_hash)
    );

    for (const result of response.results) {
      const payload = result.signed_json.payload;
      if ("wasm_hash" in payload) {
        const extPayload = payload as ExtensionPayload;
        if (!trustedHashes.has(extPayload.wasm_hash)) {
          throw new Error(
            `Untrusted wasm_hash for extension "${extPayload.extension_id}": ` +
              `${extPayload.wasm_hash}. Not found in GlobalConfig trusted_wasm_modules.`
          );
        }
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Internal
  // ---------------------------------------------------------------------------

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private async gatewayPost(gatewayUrl: string, path: string, body: unknown): Promise<any> {
    const base = stripQuery(gatewayUrl);
    const url = new URL(path, base);
    const apiKey = extractApiKey(gatewayUrl);
    if (apiKey) {
      url.searchParams.set("apikey", apiKey);
    }

    const res = await fetch(url.toString(), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(`Gateway ${path} failed: HTTP ${res.status} - ${text}`);
    }

    return res.json();
  }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

function stripQuery(url: string): string {
  const u = new URL(url);
  u.search = "";
  return u.toString().replace(/\/$/, "");
}

function extractApiKey(url: string): string | null {
  try {
    const u = new URL(url);
    return u.searchParams.get("apikey");
  } catch {
    return null;
  }
}
