// SPDX-License-Identifier: Apache-2.0

/**
 * crypto.ts のテスト
 *
 * - ワイヤーフォーマット構築・解析
 * - 方向別鍵導出（request_key ≠ response_key）
 * - E2EEフルフロー（暗号化→復号ラウ��ドトリップ）
 * - AADの不一致検出
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";

import {
  encryptPayload,
  buildPlaintext,
  decryptResponse,
  defaultCryptoProvider,
} from "../crypto";
import { x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";

describe("crypto", () => {
  describe("encryptPayload wire format", () => {
    it("生��されるペイロードがsuite_idワイヤーフォーマットに準拠する", async () => {
      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);

      const plaintext = new TextEncoder().encode("test");
      const aad = new TextEncoder().encode("/verify");

      const { payload } = await encryptPayload(teePubkey, plaintext, aad);

      // suite_id = 0x01
      assert.equal(payload[0], 0x01);
      // encap_key_len = 32 (0x0020 BE)
      assert.equal(payload[1], 0x00);
      assert.equal(payload[2], 0x20);
      // encap_key = 32 bytes
      const encapKey = payload.slice(3, 35);
      assert.equal(encapKey.length, 32);
      // nonce = 12 bytes
      const nonce = payload.slice(35, 47);
      assert.equal(nonce.length, 12);
      // ciphertext = remaining
      assert.ok(payload.length > 47);
    });

    it("毎回異なるエフェメラル鍵を生成する", async () => {
      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);
      const aad = new Uint8Array(0);

      const { payload: p1 } = await encryptPayload(teePubkey, new Uint8Array(1), aad);
      const { payload: p2 } = await encryptPayload(teePubkey, new Uint8Array(1), aad);

      // encap keys differ
      assert.notDeepEqual(p1.slice(3, 35), p2.slice(3, 35));
    });
  });

  describe("direction-separated key derivation", () => {
    it("request_keyとresponse_keyは異なる", () => {
      const sharedSecret = new Uint8Array(32).fill(0x42);
      const encapKey = new Uint8Array(32).fill(0xAA);

      const requestKey = hkdf(
        sha256, sharedSecret, encapKey,
        new TextEncoder().encode("title-request-key"), 32,
      );
      const responseKey = hkdf(
        sha256, sharedSecret, encapKey,
        new TextEncoder().encode("title-response-key"), 32,
      );

      assert.notDeepEqual(requestKey, responseKey);
      assert.equal(requestKey.length, 32);
      assert.equal(responseKey.length, 32);
    });

    it("クライアント側とTEE側で同一の鍵が導出される", () => {
      const secretKey = x25519.utils.randomPrivateKey();
      const publicKey = x25519.getPublicKey(secretKey);

      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);

      // Client: ECDH(eph_sk, tee_pk)
      const clientShared = x25519.getSharedSecret(secretKey, teePubkey);
      // TEE: ECDH(tee_sk, eph_pk)
      const teeShared = x25519.getSharedSecret(teeSecret, publicKey);

      assert.deepEqual(clientShared, teeShared);

      // Both derive same directional keys
      const encapKey = publicKey;
      const clientRequest = hkdf(sha256, clientShared, encapKey,
        new TextEncoder().encode("title-request-key"), 32);
      const teeRequest = hkdf(sha256, teeShared, encapKey,
        new TextEncoder().encode("title-request-key"), 32);
      assert.deepEqual(clientRequest, teeRequest);
    });
  });

  describe("AES-256-GCM with AAD", () => {
    it("暗号化→復号のラウンドトリップが成功する", async () => {
      const key = new Uint8Array(32).fill(0xaa);
      const plaintext = new TextEncoder().encode("Hello, Title Protocol!");
      const aad = new TextEncoder().encode("/verify");

      const { nonce, ciphertext } = await defaultCryptoProvider.encrypt(key, plaintext, aad);
      assert.equal(nonce.length, 12);
      assert.notDeepEqual(ciphertext, plaintext);

      const decrypted = await defaultCryptoProvider.decrypt(key, nonce, ciphertext, aad);
      assert.deepEqual(decrypted, plaintext);
    });

    it("AAD不一致で復号が失敗する", async () => {
      const key = new Uint8Array(32).fill(0xbb);
      const plaintext = new TextEncoder().encode("secret");
      const aad1 = new TextEncoder().encode("/verify");
      const aad2 = new TextEncoder().encode("/sign");

      const { nonce, ciphertext } = await defaultCryptoProvider.encrypt(key, plaintext, aad1);

      await assert.rejects(
        () => defaultCryptoProvider.decrypt(key, nonce, ciphertext, aad2),
      );
    });
  });

  describe("encryptPayload / decryptResponse E2EE", () => {
    it("フルフロー: encrypt → TEE decrypt → TEE encrypt response → client decrypt", async () => {
      // TEEのキーペア生成
      const teeSecret = x25519.utils.randomPrivateKey();
      const teePubkey = x25519.getPublicKey(teeSecret);

      // クライアント側: バイナリ平文を構築
      const content = new TextEncoder().encode("Hello, Title Protocol!");
      const plaintext = buildPlaintext(
        { owner_wallet: "SomeBase58Address" },
        content,
      );

      const aad = new TextEncoder().encode("/verify");

      // クライアント側: 暗号化
      const { payload, responseKey } = await encryptPayload(
        teePubkey,
        plaintext,
        aad,
      );

      // TEE側: ワイヤーフォーマット解析
      assert.equal(payload[0], 0x01); // suite_id
      const encapKeyLen = (payload[1] << 8) | payload[2];
      assert.equal(encapKeyLen, 32);
      const encapKey = payload.slice(3, 3 + encapKeyLen);
      const nonce = payload.slice(3 + encapKeyLen, 3 + encapKeyLen + 12);
      const ciphertext = payload.slice(3 + encapKeyLen + 12);

      // TEE側: KEM decapsulate + KDF
      const teeShared = x25519.getSharedSecret(teeSecret, encapKey);
      const teeRequestKey = hkdf(sha256, teeShared, encapKey,
        new TextEncoder().encode("title-request-key"), 32);
      const teeResponseKey = hkdf(sha256, teeShared, encapKey,
        new TextEncoder().encode("title-response-key"), 32);

      // TEE側: AEAD復号
      const decrypted = await defaultCryptoProvider.decrypt(
        teeRequestKey, nonce, ciphertext, aad,
      );
      assert.deepEqual(decrypted, plaintext);

      // 平文パース
      const metaLen = new DataView(decrypted.buffer, decrypted.byteOffset).getUint32(0);
      const metaJson = JSON.parse(
        new TextDecoder().decode(decrypted.slice(4, 4 + metaLen)),
      );
      assert.equal(metaJson.owner_wallet, "SomeBase58Address");

      // TEE側: レスポンスをresponse_keyで暗号化
      const responsePayload = JSON.stringify({
        results: [{ processor_id: "core-c2pa", signed_json: {} }],
      });
      const responseBytes = new TextEncoder().encode(responsePayload);
      const { nonce: respNonce, ciphertext: respCt } =
        await defaultCryptoProvider.encrypt(teeResponseKey, responseBytes, aad);

      // クライアント側: レスポンス復号（responseKeyを使用）
      assert.deepEqual(responseKey, teeResponseKey);
      const clientDecrypted = await decryptResponse(
        responseKey,
        Buffer.from(respNonce).toString("base64"),
        Buffer.from(respCt).toString("base64"),
        aad,
      );
      assert.deepEqual(clientDecrypted, responseBytes);
    });
  });

  describe("buildPlaintext", () => {
    it("正しいバイナリフォーマットを生成する", () => {
      const content = new Uint8Array([1, 2, 3, 4, 5]);
      const result = buildPlaintext(
        { owner_wallet: "W", extension_inputs: { ext: { key: "val" } } },
        content,
      );

      const metaLen = new DataView(result.buffer, result.byteOffset).getUint32(0);
      const metaJson = JSON.parse(
        new TextDecoder().decode(result.slice(4, 4 + metaLen)),
      );
      assert.equal(metaJson.owner_wallet, "W");
      assert.deepEqual(metaJson.extension_inputs, { ext: { key: "val" } });
      assert.deepEqual(result.slice(4 + metaLen), content);
    });
  });
});
