// SPDX-License-Identifier: Apache-2.0

/**
 * E2EE client-side cryptographic operations.
 *
 * Spec §6.4 — Hybrid encryption (KEM + KDF + AEAD)
 *
 * Interoperable with the Rust crates/crypto implementation:
 * - X25519 ECDH key exchange (KEM abstraction)
 * - HKDF-SHA256 direction-separated key derivation
 *   - salt: encap_key, info: "title-request-key" / "title-response-key"
 * - AES-256-GCM encryption/decryption with AAD
 *
 * Wire format: [suite_id(1B)][encap_key_len(2B BE)][encap_key][nonce][ciphertext]
 *
 * AES-GCM and Base64 operations are abstracted via CryptoProvider so that
 * platform-specific native implementations can be injected for mobile.
 */

import { x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";
import { randomBytes } from "@noble/hashes/utils";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** Suite ID: X25519-HKDF-SHA256 + AES-256-GCM. Matches Rust CryptoSuite::X25519Aes256Gcm. */
const SUITE_X25519_AES256GCM = 0x01;

/** AES-256-GCM nonce size in bytes. */
const NONCE_SIZE = 12;

/** HKDF info for request direction key. */
const HKDF_INFO_REQUEST = new TextEncoder().encode("title-request-key");

/** HKDF info for response direction key. */
const HKDF_INFO_RESPONSE = new TextEncoder().encode("title-response-key");

// ---------------------------------------------------------------------------
// CryptoProvider — pluggable AES-GCM + Base64
// ---------------------------------------------------------------------------

/**
 * Pluggable cryptographic operations for AES-256-GCM and Base64 encoding.
 *
 * The default implementation uses Web Crypto API (`crypto.subtle`) and `Buffer`.
 *
 * @example
 * ```typescript
 * const provider: CryptoProvider = {
 *   encrypt: async (key, plaintext, aad) => { ... },
 *   decrypt: async (key, nonce, ciphertext, aad) => { ... },
 *   toBase64: (bytes) => { ... },
 *   fromBase64: (str) => { ... },
 * };
 * const client = new TitleClient(globalConfig, { crypto: provider });
 * ```
 */
export interface CryptoProvider {
  /**
   * Encrypt plaintext with AES-256-GCM.
   * Must generate a random 12-byte nonce internally.
   */
  encrypt(
    key: Uint8Array,
    plaintext: Uint8Array,
    aad: Uint8Array,
  ): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }>;

  /**
   * Decrypt ciphertext with AES-256-GCM.
   */
  decrypt(
    key: Uint8Array,
    nonce: Uint8Array,
    ciphertext: Uint8Array,
    aad: Uint8Array,
  ): Promise<Uint8Array>;

  /**
   * Encode bytes to Base64 string.
   * Must produce standard Base64 (RFC 4648, with padding).
   */
  toBase64(bytes: Uint8Array): string;

  /**
   * Decode Base64 string to bytes.
   */
  fromBase64(str: string): Uint8Array;
}

// ---------------------------------------------------------------------------
// Default CryptoProvider — Web Crypto API + Buffer
// ---------------------------------------------------------------------------

/** Default CryptoProvider using Web Crypto API and Buffer. */
export const defaultCryptoProvider: CryptoProvider = {
  async encrypt(
    key: Uint8Array,
    plaintext: Uint8Array,
    aad: Uint8Array,
  ): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }> {
    const nonce = randomBytes(NONCE_SIZE);
    const cryptoKey = await crypto.subtle.importKey(
      "raw",
      key,
      { name: "AES-GCM" },
      false,
      ["encrypt"],
    );
    const buf = await crypto.subtle.encrypt(
      { name: "AES-GCM", iv: nonce, additionalData: aad },
      cryptoKey,
      plaintext,
    );
    return { nonce, ciphertext: new Uint8Array(buf) };
  },

  async decrypt(
    key: Uint8Array,
    nonce: Uint8Array,
    ciphertext: Uint8Array,
    aad: Uint8Array,
  ): Promise<Uint8Array> {
    const cryptoKey = await crypto.subtle.importKey(
      "raw",
      key,
      { name: "AES-GCM" },
      false,
      ["decrypt"],
    );
    const buf = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: nonce, additionalData: aad },
      cryptoKey,
      ciphertext,
    );
    return new Uint8Array(buf);
  },

  toBase64(bytes: Uint8Array): string {
    return Buffer.from(bytes).toString("base64");
  },

  fromBase64(str: string): Uint8Array {
    return new Uint8Array(Buffer.from(str, "base64"));
  },
};

// ---------------------------------------------------------------------------
// Internal: Key derivation
// ---------------------------------------------------------------------------

/**
 * Derive direction-separated keys from shared secret + encap key.
 * RFC 5869: HKDF with salt=encapKey, IKM=sharedSecret.
 * Matches Rust sealed_channel::derive_keys().
 */
function deriveDirectionalKeys(
  sharedSecret: Uint8Array,
  encapKey: Uint8Array,
): { requestKey: Uint8Array; responseKey: Uint8Array } {
  const requestKey = hkdf(sha256, sharedSecret, encapKey, HKDF_INFO_REQUEST, 32);
  const responseKey = hkdf(sha256, sharedSecret, encapKey, HKDF_INFO_RESPONSE, 32);
  return { requestKey, responseKey };
}

// ---------------------------------------------------------------------------
// Internal: Wire format
// ---------------------------------------------------------------------------

/**
 * Build wire format: [suite_id(1B)][encap_key_len(2B BE)][encap_key][nonce][ciphertext]
 * Matches Rust wire::build_wire().
 */
function buildWireFormat(
  suiteId: number,
  encapKey: Uint8Array,
  nonce: Uint8Array,
  ciphertext: Uint8Array,
): Uint8Array {
  const total = 3 + encapKey.length + nonce.length + ciphertext.length;
  const out = new Uint8Array(total);
  out[0] = suiteId;
  out[1] = (encapKey.length >> 8) & 0xff;
  out[2] = encapKey.length & 0xff;
  out.set(encapKey, 3);
  out.set(nonce, 3 + encapKey.length);
  out.set(ciphertext, 3 + encapKey.length + nonce.length);
  return out;
}

// ---------------------------------------------------------------------------
// High-level: encrypt / decrypt (provider-aware)
// ---------------------------------------------------------------------------

/**
 * Encrypt a client payload into a wire-format binary blob.
 * Spec §6.4.3
 *
 * Performs X25519 KEM + HKDF direction-separated key derivation + AES-256-GCM encryption.
 *
 * @param teeEncryptionPubkey - TEE X25519 public key (32 bytes)
 * @param plaintext - Bytes to encrypt (typically metadata + content binary)
 * @param aad - Additional Authenticated Data (e.g., endpoint path like "/verify")
 * @param provider - CryptoProvider (default: Web Crypto API)
 * @returns Wire-format payload and response key for decrypting TEE's response
 */
export async function encryptPayload(
  teeEncryptionPubkey: Uint8Array,
  plaintext: Uint8Array,
  aad: Uint8Array,
  provider: CryptoProvider = defaultCryptoProvider,
): Promise<{
  payload: Uint8Array;
  responseKey: Uint8Array;
}> {
  // 1. KEM: generate ephemeral X25519 key pair + ECDH
  const ephSecretKey = x25519.utils.randomPrivateKey();
  const ephPublicKey = x25519.getPublicKey(ephSecretKey);
  const sharedSecret = x25519.getSharedSecret(ephSecretKey, teeEncryptionPubkey);

  // 2. KDF: direction-separated key derivation
  const { requestKey, responseKey } = deriveDirectionalKeys(sharedSecret, ephPublicKey);

  // 3. AEAD: encrypt with request_key
  const { nonce, ciphertext } = await provider.encrypt(requestKey, plaintext, aad);

  // 4. Build wire format
  const payload = buildWireFormat(SUITE_X25519_AES256GCM, ephPublicKey, nonce, ciphertext);

  return { payload, responseKey };
}

/**
 * Decrypt an encrypted response from the TEE.
 * Spec §6.4
 *
 * @param responseKey - Response key returned by `encryptPayload()`
 * @param nonceB64 - Base64-encoded nonce from TEE response
 * @param ciphertextB64 - Base64-encoded ciphertext from TEE response
 * @param aad - Additional Authenticated Data (same endpoint path used in encrypt)
 * @param provider - CryptoProvider (default: Web Crypto API)
 */
export async function decryptResponse(
  responseKey: Uint8Array,
  nonceB64: string,
  ciphertextB64: string,
  aad: Uint8Array,
  provider: CryptoProvider = defaultCryptoProvider,
): Promise<Uint8Array> {
  const nonce = provider.fromBase64(nonceB64);
  const ciphertext = provider.fromBase64(ciphertextB64);
  return provider.decrypt(responseKey, nonce, ciphertext, aad);
}

/**
 * Build a binary plaintext from metadata and content.
 * Spec §5.1 Step 1
 *
 * Format: `[4B: metadata_len (big-endian u32)][metadata JSON][raw content bytes]`
 */
export function buildPlaintext(
  metadata: { owner_wallet: string; extension_inputs?: Record<string, unknown> },
  content: Uint8Array,
): Uint8Array {
  const metadataJson = new TextEncoder().encode(JSON.stringify(metadata));
  const metadataLen = metadataJson.length;
  const plaintext = new Uint8Array(4 + metadataLen + content.length);
  // big-endian u32
  plaintext[0] = (metadataLen >>> 24) & 0xff;
  plaintext[1] = (metadataLen >>> 16) & 0xff;
  plaintext[2] = (metadataLen >>> 8) & 0xff;
  plaintext[3] = metadataLen & 0xff;
  plaintext.set(metadataJson, 4);
  plaintext.set(content, 4 + metadataLen);
  return plaintext;
}
