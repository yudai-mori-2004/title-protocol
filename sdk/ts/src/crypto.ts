/**
 * E2EEクライアント側の暗号処理
 *
 * 仕様書 §6.4 ハイブリッド暗号化
 *
 * Rust crates/crypto と同一アルゴリズム・パラメータで相互運用可能:
 * - X25519 ECDH 鍵交換
 * - HKDF-SHA256 鍵導出（info: "title-protocol-e2ee", salt: なし）
 * - AES-256-GCM 暗号化/復号
 */

import { x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";
import { randomBytes } from "@noble/hashes/utils";

import type { EncryptedPayload } from "./types";

/** HKDF infoバイト列（Rust側: b"title-protocol-e2ee"） */
const HKDF_INFO = new TextEncoder().encode("title-protocol-e2ee");

/** エフェメラルキーペア（X25519） */
export interface EphemeralKeyPair {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

/**
 * エフェメラルX25519キーペアを生成する。
 * 仕様書 §6.4 Step 2
 */
export function generateEphemeralKeyPair(): EphemeralKeyPair {
  const secretKey = x25519.utils.randomPrivateKey();
  const publicKey = x25519.getPublicKey(secretKey);
  return { publicKey, secretKey };
}

/**
 * ECDH鍵交換で共有秘密を導出する。
 * 仕様書 §6.4 Step 3
 *
 * クライアント側: ECDH(eph_sk, tee_pk)
 * TEE側: ECDH(tee_sk, eph_pk)
 */
export function deriveSharedSecret(
  ephemeralSecretKey: Uint8Array,
  teePublicKey: Uint8Array
): Uint8Array {
  return x25519.getSharedSecret(ephemeralSecretKey, teePublicKey);
}

/**
 * HKDF-SHA256で対称鍵を導出する。
 * 仕様書 §6.4 Step 4
 *
 * Rust側と同一パラメータ:
 * - hash: SHA-256
 * - salt: なし
 * - info: "title-protocol-e2ee"
 * - 出力長: 32バイト（AES-256鍵）
 */
export function deriveSymmetricKey(sharedSecret: Uint8Array): Uint8Array {
  return hkdf(sha256, sharedSecret, undefined, HKDF_INFO, 32);
}

/**
 * AES-256-GCMでペイロードを暗号化する。
 * 仕様書 §6.4 Step 4
 *
 * nonceは12バイトのランダム値を自動生成する。
 * 返却値のEncryptedPayloadにはBase64エンコード済みのnonce/ciphertextが含まれる。
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
 * AES-256-GCMで暗号文を復号する。
 * 仕様書 §6.4 Step 9
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
 * クライアントペイロードを暗号化し、EncryptedPayload（Base64エンコード済み）を返す。
 * 仕様書 §6.4
 *
 * @param teeEncryptionPubkey - TEEのX25519公開鍵（32バイト）
 * @param plaintext - 暗号化対象のバイト列
 * @returns ephemeralPublicKey（ECDHに使用）とEncryptedPayload
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
 * TEEからの暗号化レスポンスを復号する。
 * 仕様書 §6.4 Step 9
 *
 * @param symmetricKey - encryptPayload時に導出した対称鍵
 * @param nonceB64 - Base64エンコードされたnonce
 * @param ciphertextB64 - Base64エンコードされた暗号文
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
