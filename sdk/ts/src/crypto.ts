// SPDX-License-Identifier: Apache-2.0

/**
 * E2EE client-side cryptographic operations.
 *
 * Spec §6.4 — Hybrid encryption
 *
 * Interoperable with the Rust crates/crypto implementation:
 * - X25519 ECDH key exchange
 * - HKDF-SHA256 key derivation (info: "title-protocol-e2ee", salt: none)
 * - AES-256-GCM encryption/decryption
 *
 * AES-GCM and Base64 operations are abstracted via CryptoProvider so that
 * platform-specific native implementations (e.g. expo-crypto, react-native-
 * quick-crypto) can be injected for better performance on mobile.
 */

import { x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";
import { randomBytes } from "@noble/hashes/utils";

import { ENCRYPTED_HEADER_SIZE } from "./types";

/** HKDF info bytes (Rust: b"title-protocol-e2ee"). */
const HKDF_INFO = new TextEncoder().encode("title-protocol-e2ee");

// ---------------------------------------------------------------------------
// CryptoProvider — pluggable AES-GCM + Base64
// ---------------------------------------------------------------------------

/**
 * Pluggable cryptographic operations for AES-256-GCM and Base64 encoding.
 *
 * The default implementation uses Web Crypto API (`crypto.subtle`) and `Buffer`,
 * which is fast on Node.js and browsers but slow on React Native's Hermes engine.
 *
 * To optimize for your platform, provide a custom implementation:
 *
 * @example
 * ```typescript
 * // React Native with expo-crypto
 * const provider: CryptoProvider = {
 *   encrypt: async (key, plaintext) => { ... },
 *   decrypt: async (key, nonce, ciphertext) => { ... },
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
  ): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }>;

  /**
   * Decrypt ciphertext with AES-256-GCM.
   */
  decrypt(
    key: Uint8Array,
    nonce: Uint8Array,
    ciphertext: Uint8Array,
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
  ): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }> {
    const nonce = randomBytes(12);
    const cryptoKey = await crypto.subtle.importKey(
      "raw",
      key,
      { name: "AES-GCM" },
      false,
      ["encrypt"],
    );
    const buf = await crypto.subtle.encrypt(
      { name: "AES-GCM", iv: nonce },
      cryptoKey,
      plaintext,
    );
    return { nonce, ciphertext: new Uint8Array(buf) };
  },

  async decrypt(
    key: Uint8Array,
    nonce: Uint8Array,
    ciphertext: Uint8Array,
  ): Promise<Uint8Array> {
    const cryptoKey = await crypto.subtle.importKey(
      "raw",
      key,
      { name: "AES-GCM" },
      false,
      ["decrypt"],
    );
    const buf = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: nonce },
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
// Ephemeral key pair & key derivation (always pure JS — fast for 32 bytes)
// ---------------------------------------------------------------------------

/** Ephemeral X25519 key pair. */
export interface EphemeralKeyPair {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

/**
 * Generate an ephemeral X25519 key pair.
 * Spec §6.4 Step 2
 */
export function generateEphemeralKeyPair(): EphemeralKeyPair {
  const secretKey = x25519.utils.randomPrivateKey();
  const publicKey = x25519.getPublicKey(secretKey);
  return { publicKey, secretKey };
}

/**
 * Derive a shared secret via ECDH key exchange.
 * Spec §6.4 Step 3
 *
 * Client side: ECDH(ephemeral_sk, tee_pk)
 * TEE side:    ECDH(tee_sk, ephemeral_pk)
 */
export function deriveSharedSecret(
  ephemeralSecretKey: Uint8Array,
  teePublicKey: Uint8Array
): Uint8Array {
  return x25519.getSharedSecret(ephemeralSecretKey, teePublicKey);
}

/**
 * Derive a symmetric key from a shared secret via HKDF-SHA256.
 * Spec §6.4 Step 4
 *
 * Parameters (matching Rust):
 * - hash: SHA-256
 * - salt: none
 * - info: "title-protocol-e2ee"
 * - output: 32 bytes (AES-256 key)
 */
export function deriveSymmetricKey(sharedSecret: Uint8Array): Uint8Array {
  return hkdf(sha256, sharedSecret, undefined, HKDF_INFO, 32);
}

// ---------------------------------------------------------------------------
// High-level encrypt / decrypt (provider-aware)
// ---------------------------------------------------------------------------

/**
 * Encrypt a payload with AES-256-GCM.
 * Spec §6.4 Step 4
 */
export async function encrypt(
  symmetricKey: Uint8Array,
  plaintext: Uint8Array,
  provider: CryptoProvider = defaultCryptoProvider,
): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }> {
  return provider.encrypt(symmetricKey, plaintext);
}

/**
 * Decrypt ciphertext with AES-256-GCM.
 * Spec §6.4 Step 9
 */
export async function decrypt(
  symmetricKey: Uint8Array,
  nonce: Uint8Array,
  ciphertext: Uint8Array,
  provider: CryptoProvider = defaultCryptoProvider,
): Promise<Uint8Array> {
  return provider.decrypt(symmetricKey, nonce, ciphertext);
}

/**
 * Encrypt a client payload into a binary blob.
 * Spec §6.4
 *
 * Wire format: `[32B: ephemeral_pubkey][12B: nonce][remaining: ciphertext]`
 *
 * @param teeEncryptionPubkey - TEE X25519 public key (32 bytes)
 * @param plaintext - Bytes to encrypt (typically metadata + content binary)
 * @param provider - CryptoProvider (default: Web Crypto API)
 * @returns The ephemeral key pair, derived symmetric key, and binary payload
 */
export async function encryptPayload(
  teeEncryptionPubkey: Uint8Array,
  plaintext: Uint8Array,
  provider: CryptoProvider = defaultCryptoProvider,
): Promise<{
  ephemeralKeyPair: EphemeralKeyPair;
  symmetricKey: Uint8Array;
  payload: Uint8Array;
}> {
  const ephemeralKeyPair = generateEphemeralKeyPair();
  const sharedSecret = deriveSharedSecret(
    ephemeralKeyPair.secretKey,
    teeEncryptionPubkey
  );
  const symmetricKey = deriveSymmetricKey(sharedSecret);
  const { nonce, ciphertext } = await provider.encrypt(symmetricKey, plaintext);

  // Wire format: [32B eph_pk][12B nonce][ciphertext]
  const payload = new Uint8Array(
    ENCRYPTED_HEADER_SIZE + ciphertext.length
  );
  payload.set(ephemeralKeyPair.publicKey, 0);
  payload.set(nonce, 32);
  payload.set(ciphertext, ENCRYPTED_HEADER_SIZE);

  return { ephemeralKeyPair, symmetricKey, payload };
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

/**
 * Decrypt an encrypted response from the TEE.
 * Spec §6.4 Step 9
 *
 * @param symmetricKey - Symmetric key derived during `encryptPayload()`
 * @param nonceB64 - Base64-encoded nonce
 * @param ciphertextB64 - Base64-encoded ciphertext
 * @param provider - CryptoProvider (default: Web Crypto API)
 */
export async function decryptResponse(
  symmetricKey: Uint8Array,
  nonceB64: string,
  ciphertextB64: string,
  provider: CryptoProvider = defaultCryptoProvider,
): Promise<Uint8Array> {
  const nonce = provider.fromBase64(nonceB64);
  const ciphertext = provider.fromBase64(ciphertextB64);
  return provider.decrypt(symmetricKey, nonce, ciphertext);
}
