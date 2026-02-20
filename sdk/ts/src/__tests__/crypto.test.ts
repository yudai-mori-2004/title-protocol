/**
 * crypto.ts のテスト
 *
 * - 暗号化→復号のラウンドトリップ
 * - ECDH共有秘密の対称性（クライアント側/TEE側で同一鍵が導出される）
 * - HKDF鍵導出の決定性
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";

import {
  generateEphemeralKeyPair,
  deriveSharedSecret,
  deriveSymmetricKey,
  encrypt,
  decrypt,
  encryptPayload,
  decryptResponse,
} from "../crypto";
import { x25519 } from "@noble/curves/ed25519";

describe("crypto", () => {
  describe("generateEphemeralKeyPair", () => {
    it("32バイトの公開鍵と秘密鍵を生成する", () => {
      const kp = generateEphemeralKeyPair();
      assert.equal(kp.publicKey.length, 32);
      assert.equal(kp.secretKey.length, 32);
    });

    it("毎回異なるキーペアを生成する", () => {
      const kp1 = generateEphemeralKeyPair();
      const kp2 = generateEphemeralKeyPair();
      assert.notDeepEqual(kp1.publicKey, kp2.publicKey);
      assert.notDeepEqual(kp1.secretKey, kp2.secretKey);
    });
  });

  describe("ECDH + HKDF", () => {
    it("クライアント側とTEE側で同一の共有秘密が導出される", () => {
      // クライアント側: エフェメラルキーペア
      const clientKp = generateEphemeralKeyPair();
      // TEE側: 固定キーペア（テスト用）
      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);

      // クライアント側: ECDH(eph_sk, tee_pk)
      const clientShared = deriveSharedSecret(clientKp.secretKey, teePubkey);
      // TEE側: ECDH(tee_sk, eph_pk)
      const teeShared = deriveSharedSecret(teeSecret, clientKp.publicKey);

      assert.deepEqual(clientShared, teeShared);
    });

    it("同一の共有秘密から同一の対称鍵が導出される", () => {
      const clientKp = generateEphemeralKeyPair();
      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);

      const clientShared = deriveSharedSecret(clientKp.secretKey, teePubkey);
      const teeShared = deriveSharedSecret(teeSecret, clientKp.publicKey);

      const clientKey = deriveSymmetricKey(clientShared);
      const teeKey = deriveSymmetricKey(teeShared);

      assert.deepEqual(clientKey, teeKey);
      assert.equal(clientKey.length, 32); // AES-256
    });

    it("HKDF鍵導出は決定的である", () => {
      const secret = new Uint8Array(32).fill(0x42);
      const key1 = deriveSymmetricKey(secret);
      const key2 = deriveSymmetricKey(secret);
      assert.deepEqual(key1, key2);
    });
  });

  describe("AES-256-GCM encrypt/decrypt", () => {
    it("暗号化→復号のラウンドトリップが成功する", async () => {
      const key = deriveSymmetricKey(new Uint8Array(32).fill(0xaa));
      const plaintext = new TextEncoder().encode("Hello, Title Protocol!");

      const { nonce, ciphertext } = await encrypt(key, plaintext);
      assert.equal(nonce.length, 12);
      assert.notDeepEqual(ciphertext, plaintext);

      const decrypted = await decrypt(key, nonce, ciphertext);
      assert.deepEqual(decrypted, plaintext);
    });

    it("異なる鍵での復号は失敗する", async () => {
      const key1 = deriveSymmetricKey(new Uint8Array(32).fill(0xaa));
      const key2 = deriveSymmetricKey(new Uint8Array(32).fill(0xbb));
      const plaintext = new TextEncoder().encode("secret data");

      const { nonce, ciphertext } = await encrypt(key1, plaintext);

      await assert.rejects(
        () => decrypt(key2, nonce, ciphertext),
        /OperationError/
      );
    });

    it("空のペイロードも暗号化・復号できる", async () => {
      const key = deriveSymmetricKey(new Uint8Array(32).fill(0xcc));
      const plaintext = new Uint8Array(0);

      const { nonce, ciphertext } = await encrypt(key, plaintext);
      const decrypted = await decrypt(key, nonce, ciphertext);
      assert.deepEqual(decrypted, plaintext);
    });

    it("大きなペイロードも暗号化・復号できる", async () => {
      const key = deriveSymmetricKey(new Uint8Array(32).fill(0xdd));
      const plaintext = new Uint8Array(1024 * 1024); // 1MB
      for (let i = 0; i < plaintext.length; i++) {
        plaintext[i] = i % 256;
      }

      const { nonce, ciphertext } = await encrypt(key, plaintext);
      const decrypted = await decrypt(key, nonce, ciphertext);
      assert.deepEqual(decrypted, plaintext);
    });
  });

  describe("encryptPayload / decryptResponse", () => {
    it("E2EEフルフロー: encrypt → Base64 → decrypt", async () => {
      // TEEのキーペア生成
      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);

      const payload = JSON.stringify({
        owner_wallet: "SomeBase58Address",
        content: "SGVsbG8=", // "Hello" in Base64
      });
      const payloadBytes = new TextEncoder().encode(payload);

      // クライアント側: 暗号化
      const { symmetricKey, encryptedPayload } = await encryptPayload(
        teePubkey,
        payloadBytes
      );

      // TEE側: 同一の対称鍵を導出
      const ephPubkeyBytes = Buffer.from(
        encryptedPayload.ephemeral_pubkey,
        "base64"
      );
      const teeShared = deriveSharedSecret(teeSecret, ephPubkeyBytes);
      const teeSymmetricKey = deriveSymmetricKey(teeShared);
      assert.deepEqual(teeSymmetricKey, symmetricKey);

      // TEE側: 復号
      const teeDecrypted = await decryptResponse(
        teeSymmetricKey,
        encryptedPayload.nonce,
        encryptedPayload.ciphertext
      );
      assert.deepEqual(teeDecrypted, payloadBytes);

      // TEE側: レスポンスを同一鍵で暗号化して返す
      const responsePayload = JSON.stringify({
        results: [{ processor_id: "core-c2pa", signed_json: {} }],
      });
      const responseBytes = new TextEncoder().encode(responsePayload);
      const { nonce: respNonce, ciphertext: respCt } = await encrypt(
        teeSymmetricKey,
        responseBytes
      );

      // クライアント側: レスポンス復号
      const clientDecrypted = await decryptResponse(
        symmetricKey,
        Buffer.from(respNonce).toString("base64"),
        Buffer.from(respCt).toString("base64")
      );
      assert.deepEqual(clientDecrypted, responseBytes);
    });
  });
});
