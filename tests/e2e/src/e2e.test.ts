/**
 * Title Protocol E2Eインテグレーションテスト
 *
 * 前提条件:
 * - docker compose up -d でサービスが起動済み
 * - ./scripts/setup-local.sh でGlobal Config初期化済み
 * - cargo run --example gen_fixture -p title-core -- tests/e2e/fixtures でフィクスチャ生成済み
 *
 * テスト実行:
 *   cd tests/e2e && npm install && npm run build && npm test
 */

import { describe, it, before, after } from "node:test";
import * as assert from "node:assert/strict";
import { Connection, Keypair, Transaction } from "@solana/web3.js";

import {
  SOLANA_RPC,
  GATEWAY_URL,
  TEE_URL,
  waitForAllServices,
  createFundedWallet,
  TestStorageServer,
  TestStorage,
  loadFixture,
  loadTeeInfo,
  setupClient,
  sleep,
  MockWallet,
} from "./helpers";

import {
  TitleClient,
  encryptPayload,
  decryptResponse,
  type VerifyResponse,
} from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// 共有状態
// ---------------------------------------------------------------------------

let storageServer: TestStorageServer;
let storage: TestStorage;
let client: TitleClient;
let wallet: MockWallet;

// ---------------------------------------------------------------------------
// セットアップ / ティアダウン
// ---------------------------------------------------------------------------

describe("E2E Integration Tests", () => {
  before(async () => {
    // サービスの起動待ち
    await waitForAllServices();

    // ストレージサーバー起動（signed_json保管用）
    storageServer = new TestStorageServer();
    await storageServer.start();
    storage = new TestStorage();

    // クライアントセットアップ
    client = await setupClient();

    // テスト用ウォレット作成
    wallet = await createFundedWallet();
  });

  after(async () => {
    if (storageServer) {
      await storageServer.stop();
    }
  });

  // -------------------------------------------------------------------------
  // テスト1: サービスヘルスチェック
  // -------------------------------------------------------------------------
  describe("Service Health", () => {
    it("Gateway /.well-known/title-node-info が応答する", async () => {
      const res = await fetch(
        `${GATEWAY_URL}/.well-known/title-node-info`
      );
      assert.equal(res.ok, true);
      const info = (await res.json()) as {
        signing_pubkey: string;
        supported_extensions: string[];
      };
      assert.ok(info.signing_pubkey, "signing_pubkey が空です");
    });

    it("Solana RPC が応答する", async () => {
      const connection = new Connection(SOLANA_RPC, "confirmed");
      const slot = await connection.getSlot();
      assert.ok(slot > 0, `Solana slot が不正: ${slot}`);
    });

    it("TEE情報が読み込める", () => {
      const teeInfo = loadTeeInfo();
      assert.ok(teeInfo.signing_pubkey, "signing_pubkey が空です");
      assert.ok(
        teeInfo.encryption_pubkey,
        "encryption_pubkey が空です"
      );
      assert.ok(teeInfo.tree_address, "tree_address が空です");
    });
  });

  // -------------------------------------------------------------------------
  // テスト2: Gateway API
  // -------------------------------------------------------------------------
  describe("Gateway API", () => {
    it("POST /upload-url で署名付きURLが発行される", async () => {
      const res = await fetch(`${GATEWAY_URL}/upload-url`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content_size: 1024,
          content_type: "application/json",
        }),
      });
      const body = await res.json() as {
        upload_url: string;
        download_url: string;
        expires_at: number;
      };
      assert.equal(res.ok, true, `HTTP ${res.status}`);
      assert.ok(body.upload_url, "upload_url が空です");
      assert.ok(body.download_url, "download_url が空です");
      assert.ok(body.expires_at > 0, "expires_at が不正です");
    });
  });

  // -------------------------------------------------------------------------
  // テスト3: E2EE暗号化 → /verify → 復号
  // -------------------------------------------------------------------------
  describe("Verify Flow (E2EE)", () => {
    it("暗号化されたC2PAコンテンツを/verifyに送信し、復号されたsigned_jsonを取得できる", async () => {
      const teeInfo = loadTeeInfo();
      const content = loadFixture("signed.jpg");

      // Step 1: ペイロード構築
      const clientPayload = {
        owner_wallet: wallet.publicKey.toBase58(),
        content: Buffer.from(content).toString("base64"),
      };
      const payloadBytes = new TextEncoder().encode(
        JSON.stringify(clientPayload)
      );

      // Step 2: 暗号化（ECDH + HKDF + AES-GCM）
      const teeEncPubkey = Buffer.from(
        teeInfo.encryption_pubkey,
        "base64"
      );
      const { symmetricKey, encryptedPayload } = await encryptPayload(
        teeEncPubkey,
        payloadBytes
      );

      // Step 3: Temporary Storageにアップロード
      const uploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content_size: JSON.stringify(encryptedPayload).length,
          content_type: "application/json",
        }),
      });
      assert.equal(uploadUrlRes.ok, true);
      const { upload_url, download_url } = (await uploadUrlRes.json()) as {
        upload_url: string;
        download_url: string;
      };

      const putRes = await fetch(upload_url, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(encryptedPayload),
      });
      assert.equal(putRes.ok, true, `Storage PUT failed: HTTP ${putRes.status}`);

      // Step 4: /verify 呼び出し
      const verifyRes = await fetch(`${GATEWAY_URL}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          download_url,
          processor_ids: ["core-c2pa"],
        }),
      });
      assert.equal(
        verifyRes.ok,
        true,
        `/verify failed: HTTP ${verifyRes.status} - ${await verifyRes.clone().text()}`
      );

      const encryptedResponse = (await verifyRes.json()) as {
        nonce: string;
        ciphertext: string;
      };
      assert.ok(encryptedResponse.nonce, "nonce が空です");
      assert.ok(encryptedResponse.ciphertext, "ciphertext が空です");

      // Step 5: レスポンス復号
      const decryptedBytes = await decryptResponse(
        symmetricKey,
        encryptedResponse.nonce,
        encryptedResponse.ciphertext
      );
      const verifyResponse: VerifyResponse = JSON.parse(
        new TextDecoder().decode(decryptedBytes)
      );

      // Step 6: 結果検証
      assert.ok(
        verifyResponse.results.length > 0,
        "results が空です"
      );
      assert.equal(
        verifyResponse.results[0].processor_id,
        "core-c2pa"
      );

      const signedJson = verifyResponse.results[0].signed_json;
      assert.ok(signedJson, "signed_json が空です");

      // signed_json の構造を検証
      // NOTE: Rust側の SignedJson.core は #[serde(flatten)] のため、
      // JSONではトップレベルにフラット化される（core: {} ネストなし）
      const sjAny = signedJson as unknown as Record<string, unknown>;
      assert.ok(sjAny.protocol, "protocol が空です");
      assert.ok(sjAny.tee_pubkey, "tee_pubkey が空です");
      assert.ok(sjAny.tee_signature, "tee_signature が空です");

      const payload = sjAny.payload as unknown as Record<string, unknown>;
      assert.ok(payload.content_hash, "content_hash が空です");
      assert.equal(payload.content_type, "image/jpeg");
      assert.equal(
        payload.creator_wallet,
        wallet.publicKey.toBase58()
      );
    });
  });

  // -------------------------------------------------------------------------
  // テスト4: /verify → /sign フルフロー
  // -------------------------------------------------------------------------
  describe("Sign Flow", () => {
    it("signed_jsonをストレージに保存し、/signで部分署名済みTXを取得できる", async () => {
      const teeInfo = loadTeeInfo();
      const content = loadFixture("signed.jpg");

      // /verify フロー（テスト3と同じ）
      const clientPayload = {
        owner_wallet: wallet.publicKey.toBase58(),
        content: Buffer.from(content).toString("base64"),
      };
      const payloadBytes = new TextEncoder().encode(
        JSON.stringify(clientPayload)
      );
      const teeEncPubkey = Buffer.from(
        teeInfo.encryption_pubkey,
        "base64"
      );
      const { symmetricKey, encryptedPayload } = await encryptPayload(
        teeEncPubkey,
        payloadBytes
      );

      // Temporary Storageにアップロード
      const uploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content_size: JSON.stringify(encryptedPayload).length,
          content_type: "application/json",
        }),
      });
      const { upload_url, download_url } = (await uploadUrlRes.json()) as {
        upload_url: string;
        download_url: string;
      };
      await fetch(upload_url, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(encryptedPayload),
      });

      // /verify
      const verifyRes = await fetch(`${GATEWAY_URL}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          download_url,
          processor_ids: ["core-c2pa"],
        }),
      });
      assert.equal(verifyRes.ok, true, `/verify failed: ${verifyRes.status}`);

      const encResp = (await verifyRes.json()) as {
        nonce: string;
        ciphertext: string;
      };
      const decrypted = await decryptResponse(
        symmetricKey,
        encResp.nonce,
        encResp.ciphertext
      );
      const verifyResponse: VerifyResponse = JSON.parse(
        new TextDecoder().decode(decrypted)
      );

      // signed_jsonをテストストレージに保存
      const signedJsonBytes = new TextEncoder().encode(
        JSON.stringify(verifyResponse.results[0].signed_json)
      );
      const signedJsonUri = await storage.upload(
        signedJsonBytes,
        "application/json"
      );
      assert.ok(signedJsonUri, "signedJsonUri が空です");

      // /sign 呼び出し
      const connection = new Connection(SOLANA_RPC, "confirmed");
      const { blockhash } = await connection.getLatestBlockhash();

      const signRes = await fetch(`${GATEWAY_URL}/sign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          recent_blockhash: blockhash,
          requests: [{ signed_json_uri: signedJsonUri }],
        }),
      });
      assert.equal(
        signRes.ok,
        true,
        `/sign failed: HTTP ${signRes.status} - ${await signRes.clone().text()}`
      );

      const signResponse = (await signRes.json()) as {
        partial_txs: string[];
      };
      assert.equal(signResponse.partial_txs.length, 1);

      // partial_tx が有効なBase64でTransactionにデコード可能
      const txBytes = Buffer.from(signResponse.partial_txs[0], "base64");
      const tx = Transaction.from(txBytes);
      assert.ok(
        tx.signatures.length >= 1,
        "partial_txに署名がありません"
      );
      assert.ok(
        tx.instructions.length >= 1,
        "partial_txに命令がありません"
      );
    });
  });

  // -------------------------------------------------------------------------
  // テスト5: 来歴グラフ（ingredients付きコンテンツ）
  // -------------------------------------------------------------------------
  describe("Provenance Graph", () => {
    it("ingredients付きコンテンツの来歴グラフが構築される", async () => {
      const teeInfo = loadTeeInfo();
      let content: Uint8Array;
      try {
        content = loadFixture("with_ingredients.jpg");
      } catch {
        // フィクスチャがない場合はスキップ
        console.log("    SKIP: with_ingredients.jpg フィクスチャが見つかりません");
        return;
      }

      // /verify フロー
      const clientPayload = {
        owner_wallet: wallet.publicKey.toBase58(),
        content: Buffer.from(content).toString("base64"),
      };
      const payloadBytes = new TextEncoder().encode(
        JSON.stringify(clientPayload)
      );
      const teeEncPubkey = Buffer.from(
        teeInfo.encryption_pubkey,
        "base64"
      );
      const { symmetricKey, encryptedPayload } = await encryptPayload(
        teeEncPubkey,
        payloadBytes
      );

      const uploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content_size: JSON.stringify(encryptedPayload).length,
          content_type: "application/json",
        }),
      });
      const { upload_url, download_url } = (await uploadUrlRes.json()) as {
        upload_url: string;
        download_url: string;
      };
      await fetch(upload_url, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(encryptedPayload),
      });

      const verifyRes = await fetch(`${GATEWAY_URL}/verify`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          download_url,
          processor_ids: ["core-c2pa"],
        }),
      });
      assert.equal(verifyRes.ok, true, `/verify failed: ${verifyRes.status}`);

      const encResp = (await verifyRes.json()) as {
        nonce: string;
        ciphertext: string;
      };
      const decrypted = await decryptResponse(
        symmetricKey,
        encResp.nonce,
        encResp.ciphertext
      );
      const verifyResponse: VerifyResponse = JSON.parse(
        new TextDecoder().decode(decrypted)
      );

      // 来歴グラフの検証
      const signedJson = verifyResponse.results[0]
        .signed_json as unknown as Record<string, unknown>;
      const payload = signedJson.payload as unknown as Record<string, unknown>;
      const nodes = payload.nodes as Array<{
        id: string;
        type: string;
      }>;
      const links = payload.links as Array<{
        source: string;
        target: string;
        role: string;
      }>;

      // ingredients付きなので、ノードが2つ以上（finalノード + ingredientノード）
      assert.ok(
        nodes.length >= 2,
        `ノード数が不足: ${nodes.length} (期待: >= 2)`
      );
      assert.ok(
        nodes.some((n) => n.type === "final"),
        "finalノードがありません"
      );
      assert.ok(
        nodes.some((n) => n.type === "ingredient"),
        "ingredientノードがありません"
      );

      // リンクが存在する
      assert.ok(links.length >= 1, "リンクがありません");
    });
  });

  // -------------------------------------------------------------------------
  // テスト6: 鍵ローテーション拒否
  // -------------------------------------------------------------------------
  describe("Key Rotation Rejection", () => {
    it("異なるTEEインスタンスの signed_json は /sign で拒否される", async () => {
      // テスト用のダミーsigned_jsonを生成（TEEの鍵とは異なるキーで署名）
      const { ed25519 } = await import("@noble/curves/ed25519");
      const fakePrivKey = ed25519.utils.randomPrivateKey();
      const fakePubKey = ed25519.getPublicKey(fakePrivKey);

      // Base58エンコード
      const bs58Module = await import("bs58");
      const fakePubKeyB58 = bs58Module.default.encode(
        Buffer.from(fakePubKey)
      );

      // ダミーペイロード
      const payload = {
        content_hash: "0x" + "ab".repeat(32),
        content_type: "image/jpeg",
        creator_wallet: wallet.publicKey.toBase58(),
        nodes: [{ id: "0xtest", type: "final" }],
        links: [],
      };
      const attributes = [
        { trait_type: "protocol", value: "Title-v1" },
        { trait_type: "content_hash", value: "0x" + "ab".repeat(32) },
      ];

      // 署名（偽TEE鍵で）
      const signTarget = JSON.stringify({ payload, attributes });
      const signBytes = new TextEncoder().encode(signTarget);
      const signature = ed25519.sign(signBytes, fakePrivKey);

      const fakeSignedJson = {
        core: {
          protocol: "Title-v1",
          tee_type: "mock",
          tee_pubkey: fakePubKeyB58,
          tee_signature: Buffer.from(signature).toString("base64"),
          tee_attestation: Buffer.from("fake").toString("base64"),
        },
        payload,
        attributes,
      };

      // テストストレージにアップロード
      const jsonBytes = new TextEncoder().encode(
        JSON.stringify(fakeSignedJson)
      );
      const uri = await storage.upload(jsonBytes, "application/json");

      // /sign 呼び出し → 403 Forbidden が期待される
      const connection = new Connection(SOLANA_RPC, "confirmed");
      const { blockhash } = await connection.getLatestBlockhash();

      const signRes = await fetch(`${GATEWAY_URL}/sign`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          recent_blockhash: blockhash,
          requests: [{ signed_json_uri: uri }],
        }),
      });

      // TEEが署名検証に失敗して拒否することを確認
      assert.equal(
        signRes.ok,
        false,
        "偽signed_jsonが受け入れられてしまいました"
      );
      // TEEが403を返し、Gatewayが502で中継する
      assert.ok(
        [403, 502].includes(signRes.status),
        `期待: 403 or 502, 実際: ${signRes.status}`
      );
    });
  });

  // -------------------------------------------------------------------------
  // テスト7: 重複コンテンツの検証
  // -------------------------------------------------------------------------
  describe("Duplicate Content", () => {
    it("同一コンテンツを2回/verifyすると同じcontent_hashが返る", async () => {
      const teeInfo = loadTeeInfo();
      const content = loadFixture("signed.jpg");

      // 共通の暗号化ヘルパー
      async function doVerify(): Promise<string> {
        const clientPayload = {
          owner_wallet: wallet.publicKey.toBase58(),
          content: Buffer.from(content).toString("base64"),
        };
        const payloadBytes = new TextEncoder().encode(
          JSON.stringify(clientPayload)
        );
        const teeEncPubkey = Buffer.from(
          teeInfo.encryption_pubkey,
          "base64"
        );
        const { symmetricKey, encryptedPayload } = await encryptPayload(
          teeEncPubkey,
          payloadBytes
        );

        const uploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            content_size: JSON.stringify(encryptedPayload).length,
            content_type: "application/json",
          }),
        });
        const { upload_url, download_url } =
          (await uploadUrlRes.json()) as {
            upload_url: string;
            download_url: string;
          };
        await fetch(upload_url, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(encryptedPayload),
        });

        const verifyRes = await fetch(`${GATEWAY_URL}/verify`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            download_url,
            processor_ids: ["core-c2pa"],
          }),
        });
        assert.equal(verifyRes.ok, true);

        const encResp = (await verifyRes.json()) as {
          nonce: string;
          ciphertext: string;
        };
        const decrypted = await decryptResponse(
          symmetricKey,
          encResp.nonce,
          encResp.ciphertext
        );
        const resp: VerifyResponse = JSON.parse(
          new TextDecoder().decode(decrypted)
        );
        const sj = resp.results[0].signed_json as unknown as Record<string, unknown>;
        const p = sj.payload as unknown as Record<string, unknown>;
        return p.content_hash as string;
      }

      const hash1 = await doVerify();
      const hash2 = await doVerify();

      assert.ok(hash1.startsWith("0x"), "content_hash が 0x で始まりません");
      assert.equal(
        hash1,
        hash2,
        "同一コンテンツのcontent_hashが一致しません"
      );
    });
  });
});
