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
 */

import { x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";
import { randomBytes } from "@noble/hashes/utils";

import type { EncryptedPayload } from "./types";

/** HKDF info bytes (Rust: b"title-protocol-e2ee"). */
const HKDF_INFO = new TextEncoder().encode("title-protocol-e2ee");

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

/**
 * Encrypt a payload with AES-256-GCM.
 * Spec §6.4 Step 4
 *
 * A 12-byte random nonce is generated automatically.
 */
export async function encrypt(
  symmetricKey: Uint8Array,
  plaintext: Uint8Array
): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }> {
  const nonce = randomBytes(12);
  const key = await crypto.subtle.importKey(
    "raw",
    symmetricKey,
    { name: "AES-GCM" },
    false,
    ["encrypt"]
  );
  const ciphertextBuf = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv: nonce },
    key,
    plaintext
  );
  return { nonce, ciphertext: new Uint8Array(ciphertextBuf) };
}

/**
 * Decrypt ciphertext with AES-256-GCM.
 * Spec §6.4 Step 9
 */
export async function decrypt(
  symmetricKey: Uint8Array,
  nonce: Uint8Array,
  ciphertext: Uint8Array
): Promise<Uint8Array> {
  const key = await crypto.subtle.importKey(
    "raw",
    symmetricKey,
    { name: "AES-GCM" },
    false,
    ["decrypt"]
  );
  const plaintextBuf = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: nonce },
    key,
    ciphertext
  );
  return new Uint8Array(plaintextBuf);
}

/**
 * Encrypt a client payload and return Base64-encoded `EncryptedPayload`.
 * Spec §6.4
 *
 * @param teeEncryptionPubkey - TEE X25519 public key (32 bytes)
 * @param plaintext - Bytes to encrypt
 * @returns The ephemeral key pair, derived symmetric key, and encrypted payload
 */
export async function encryptPayload(
  teeEncryptionPubkey: Uint8Array,
  plaintext: Uint8Array
): Promise<{
  ephemeralKeyPair: EphemeralKeyPair;
  symmetricKey: Uint8Array;
  encryptedPayload: EncryptedPayload;
}> {
  const ephemeralKeyPair = generateEphemeralKeyPair();
  const sharedSecret = deriveSharedSecret(
    ephemeralKeyPair.secretKey,
    teeEncryptionPubkey
  );
  const symmetricKey = deriveSymmetricKey(sharedSecret);
  const { nonce, ciphertext } = await encrypt(symmetricKey, plaintext);

  const toBase64 = (bytes: Uint8Array): string =>
    Buffer.from(bytes).toString("base64");

  return {
    ephemeralKeyPair,
    symmetricKey,
    encryptedPayload: {
      ephemeral_pubkey: toBase64(ephemeralKeyPair.publicKey),
      nonce: toBase64(nonce),
      ciphertext: toBase64(ciphertext),
    },
  };
}

/**
 * Decrypt an encrypted response from the TEE.
 * Spec §6.4 Step 9
 *
 * @param symmetricKey - Symmetric key derived during `encryptPayload()`
 * @param nonceB64 - Base64-encoded nonce
 * @param ciphertextB64 - Base64-encoded ciphertext
 */
export async function decryptResponse(
  symmetricKey: Uint8Array,
  nonceB64: string,
  ciphertextB64: string
): Promise<Uint8Array> {
  const nonce = Buffer.from(nonceB64, "base64");
  const ciphertext = Buffer.from(ciphertextB64, "base64");
  return decrypt(symmetricKey, nonce, ciphertext);
}
