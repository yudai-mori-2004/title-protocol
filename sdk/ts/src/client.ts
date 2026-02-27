// SPDX-License-Identifier: Apache-2.0

/**
 * TitleClient — Main SDK class.
 *
 * Spec §6.7
 *
 * TEE nodes are managed as a flat URL array.
 * Each URL follows the format `https://<gateway-host>?apikey=<key>`.
 * The SDK randomly selects a node from the array, but once an encrypted
 * upload is performed, the session is pinned to that node (affinity).
 */

import type {
  GlobalConfig,
  TrustedTeeNode,
  TrustedWasmModule,
  VerifyRequest,
  SignRequest,
  SignResponse,
  EncryptedPayload,
  NodeInfo,
} from "./types";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/** TitleClient initialization options. */
export interface TitleClientConfig {
  /**
   * Flat array of TEE node Gateway URLs.
   * Each URL may include an API key: `https://gateway.example.com?apikey=xxx`.
   * The SDK randomly selects one node from this array.
   */
  teeNodes: string[];

  /** Solana RPC URL. */
  solanaRpcUrl: string;

  /**
   * GlobalConfig data (injected).
   * In production, this should be fetched from the on-chain GlobalConfig PDA
   * via Solana RPC. The SDK currently accepts it as a constructor parameter
   * for flexibility.
   */
  globalConfig: GlobalConfig;
}

// ---------------------------------------------------------------------------
// Node Session (affinity management)
// ---------------------------------------------------------------------------

/**
 * Session with a specific TEE node.
 * After encrypted upload, use the same session for verify/sign calls
 * to maintain node affinity.
 */
export interface TeeSession {
  /** Gateway URL for this session. */
  gatewayUrl: string;
  /** TEE X25519 encryption public key (Base64). */
  encryptionPubkey: string;
  /** TEE Ed25519 signing public key (Base58). */
  signingPubkey: string;
}

// ---------------------------------------------------------------------------
// TitleClient
// ---------------------------------------------------------------------------

export class TitleClient {
  readonly config: TitleClientConfig;

  constructor(config: TitleClientConfig) {
    if (config.teeNodes.length === 0) {
      throw new Error("teeNodes must contain at least one URL");
    }
    this.config = config;
  }

  /**
   * Select a random TEE node and start a session.
   * Call this before performing an encrypted upload.
   * Pass the returned TeeSession to subsequent verify/sign calls
   * to ensure node affinity.
   *
   * Spec §6.7
   */
  async selectNode(): Promise<TeeSession> {
    const gatewayUrl = this.pickRandomNode();

    const nodeInfo = await this.getNodeInfo(gatewayUrl);
    const teeNode = this.findTeeNodeBySigningPubkey(nodeInfo.signing_pubkey);

    return {
      gatewayUrl,
      encryptionPubkey: teeNode.encryption_pubkey,
      signingPubkey: teeNode.signing_pubkey,
    };
  }

  /**
   * Fetch `/.well-known/title-node-info` from a Gateway.
   * Spec §6.2
   */
  async getNodeInfo(gatewayUrl: string): Promise<NodeInfo> {
    const url = new URL("/.well-known/title-node-info", stripQuery(gatewayUrl));
    const apiKey = extractApiKey(gatewayUrl);
    if (apiKey) {
      url.searchParams.set("apikey", apiKey);
    }

    const res = await fetch(url.toString());
    if (!res.ok) {
      throw new Error(
        `Failed to fetch node info: HTTP ${res.status} ${await res.text()}`
      );
    }
    return (await res.json()) as NodeInfo;
  }

  /**
   * Get a signed upload URL for temporary storage.
   * Spec §6.2 POST /upload-url
   */
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

  /**
   * Upload an encrypted payload to temporary storage.
   * Spec §6.7
   *
   * @returns Download URL for the TEE to fetch the payload.
   */
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

  /**
   * Call the `/verify` endpoint.
   * Spec §6.2
   *
   * The response body is AES-GCM encrypted. Use `decryptResponse()` from
   * the crypto module with the symmetric key from the encryption step.
   */
  async verify(
    gatewayUrl: string,
    request: VerifyRequest
  ): Promise<{ nonce: string; ciphertext: string }> {
    const res = await this.gatewayPost(gatewayUrl, "/verify", request);
    return { nonce: res.nonce, ciphertext: res.ciphertext };
  }

  /**
   * Call the `/sign` endpoint.
   * Spec §6.2
   */
  async sign(
    gatewayUrl: string,
    request: SignRequest
  ): Promise<SignResponse> {
    return await this.gatewayPost(gatewayUrl, "/sign", request);
  }

  /**
   * Call the `/sign-and-mint` endpoint (Gateway-assisted minting).
   * Spec §6.2
   */
  async signAndMint(
    gatewayUrl: string,
    request: SignRequest
  ): Promise<{ txSignatures: string[] }> {
    const res = await this.gatewayPost(gatewayUrl, "/sign-and-mint", request);
    return { txSignatures: res.tx_signatures };
  }

  // --- GlobalConfig accessors ---

  /**
   * Get trusted WASM modules from GlobalConfig.
   * Spec §5.2 Step 1
   */
  getTrustedWasmModules(): TrustedWasmModule[] {
    return this.config.globalConfig.trusted_wasm_modules;
  }

  /**
   * Get Core collection mint address from GlobalConfig.
   * Spec §5.2 Step 1
   */
  getCoreCollectionMint(): string {
    return this.config.globalConfig.core_collection_mint;
  }

  /**
   * Get Extension collection mint address from GlobalConfig.
   * Spec §5.2 Step 1
   */
  getExtCollectionMint(): string {
    return this.config.globalConfig.ext_collection_mint;
  }

  /**
   * Get trusted TEE nodes from GlobalConfig.
   * Spec §5.2 Step 1
   */
  getTrustedTeeNodes(): TrustedTeeNode[] {
    return this.config.globalConfig.trusted_tee_nodes;
  }

  // --- Internal helpers ---

  /** Randomly select a TEE node URL. */
  private pickRandomNode(): string {
    const idx = Math.floor(Math.random() * this.config.teeNodes.length);
    return this.config.teeNodes[idx];
  }

  /** Look up a TeeNode in GlobalConfig by signing_pubkey. */
  private findTeeNodeBySigningPubkey(signingPubkey: string): TrustedTeeNode {
    const node = this.config.globalConfig.trusted_tee_nodes.find(
      (n) => n.signing_pubkey === signingPubkey
    );
    if (!node) {
      throw new Error(
        `TEE node with signing_pubkey=${signingPubkey} not found in GlobalConfig`
      );
    }
    return node;
  }

  /** Send a POST request to a Gateway endpoint. */
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

/** Strip query parameters from a URL and return the base URL. */
function stripQuery(url: string): string {
  const u = new URL(url);
  u.search = "";
  return u.toString().replace(/\/$/, "");
}

/** Extract the `apikey` query parameter from a URL. */
function extractApiKey(url: string): string | null {
  try {
    const u = new URL(url);
    return u.searchParams.get("apikey");
  } catch {
    return null;
  }
}
