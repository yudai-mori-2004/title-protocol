#!/usr/bin/env node
/**
 * Title Protocol Devnet E2Eテストスクリプト
 *
 * ローカルマシンからEC2上のGatewayに対して、
 * C2PAコンテンツの登録→cNFTミント全フローをテストする。
 *
 * 使い方:
 *   cd scripts && npm install
 *   node test-devnet.mjs --gateway http://<EC2_IP>:3000
 *
 * 環境変数:
 *   GATEWAY_URL           - Gateway URL (default: http://localhost:3000)
 *   SOLANA_RPC_URL        - Solana RPC (default: https://api.devnet.solana.com)
 *   TEE_ENCRYPTION_PUBKEY - TEE暗号化公開鍵 (Base64, node-infoから自動取得)
 */

import { x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";
import { randomBytes } from "@noble/hashes/utils";
import {
  Connection,
  Keypair,
  Transaction,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import { webcrypto } from "crypto";
import { readFileSync, existsSync } from "fs";
import { homedir } from "os";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PROJECT_ROOT = dirname(__dirname);

// ---------------------------------------------------------------------------
// 引数・設定
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
function getArg(name, defaultVal) {
  const idx = args.indexOf(`--${name}`);
  if (idx >= 0 && idx + 1 < args.length) return args[idx + 1];
  return defaultVal;
}

const GATEWAY_URL = getArg("gateway", process.env.GATEWAY_URL || "http://localhost:3000");
const SOLANA_RPC_URL = getArg("rpc", process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com");
const FIXTURE_PATH = getArg("fixture", join(PROJECT_ROOT, "tests/e2e/fixtures/signed.jpg"));

// HKDF info（Rust側と同一）
const HKDF_INFO = new TextEncoder().encode("title-protocol-e2ee");

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

function loadKeypair() {
  const keyPath = join(homedir(), ".config", "solana", "id.json");
  if (!existsSync(keyPath)) {
    console.error(`ERROR: Solana keypair not found: ${keyPath}`);
    process.exit(1);
  }
  const raw = JSON.parse(readFileSync(keyPath, "utf-8"));
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

async function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// E2EE暗号関数（SDK crypto.ts と同一アルゴリズム）
// ---------------------------------------------------------------------------

function generateEphemeralKeyPair() {
  const secretKey = x25519.utils.randomPrivateKey();
  const publicKey = x25519.getPublicKey(secretKey);
  return { publicKey, secretKey };
}

function deriveSharedSecret(ephSk, teePk) {
  return x25519.getSharedSecret(ephSk, teePk);
}

function deriveSymmetricKey(sharedSecret) {
  return hkdf(sha256, sharedSecret, undefined, HKDF_INFO, 32);
}

async function aesGcmEncrypt(key, plaintext) {
  const nonce = randomBytes(12);
  const cryptoKey = await crypto.subtle.importKey(
    "raw", key, { name: "AES-GCM" }, false, ["encrypt"]
  );
  const ciphertext = new Uint8Array(
    await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonce }, cryptoKey, plaintext)
  );
  return { nonce, ciphertext };
}

async function aesGcmDecrypt(key, nonce, ciphertext) {
  const cryptoKey = await crypto.subtle.importKey(
    "raw", key, { name: "AES-GCM" }, false, ["decrypt"]
  );
  return new Uint8Array(
    await crypto.subtle.decrypt({ name: "AES-GCM", iv: nonce }, cryptoKey, ciphertext)
  );
}

function toBase64(bytes) {
  return Buffer.from(bytes).toString("base64");
}

function fromBase64(str) {
  return Buffer.from(str, "base64");
}

// ---------------------------------------------------------------------------
// メイン
// ---------------------------------------------------------------------------

async function main() {
  console.log("=== Title Protocol Devnet E2Eテスト ===\n");

  // -----------------------------------------------------------------------
  // Step 0: 事前確認
  // -----------------------------------------------------------------------
  console.log("[Step 0] 事前確認...");
  console.log(`  Gateway: ${GATEWAY_URL}`);
  console.log(`  Solana RPC: ${SOLANA_RPC_URL}`);

  // Gateway ヘルスチェック
  let nodeInfo;
  try {
    const res = await fetch(`${GATEWAY_URL}/.well-known/title-node-info`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    nodeInfo = await res.json();
    console.log(`  Gateway signing_pubkey: ${nodeInfo.signing_pubkey}`);
    console.log(`  supported_extensions: ${nodeInfo.supported_extensions.join(", ")}`);
  } catch (e) {
    console.error(`  ERROR: Gateway に接続できません: ${e.message}`);
    process.exit(1);
  }

  // Solana RPC チェック
  const connection = new Connection(SOLANA_RPC_URL, "confirmed");
  try {
    const slot = await connection.getSlot();
    console.log(`  Solana slot: ${slot}`);
  } catch (e) {
    console.error(`  ERROR: Solana RPC に接続できません: ${e.message}`);
    process.exit(1);
  }

  // ウォレット
  const wallet = loadKeypair();
  console.log(`  Wallet: ${wallet.publicKey.toBase58()}`);
  const balance = await connection.getBalance(wallet.publicKey);
  console.log(`  Balance: ${balance / LAMPORTS_PER_SOL} SOL`);
  if (balance < 0.01 * LAMPORTS_PER_SOL) {
    console.log("  Airdrop中...");
    try {
      const sig = await connection.requestAirdrop(wallet.publicKey, 2 * LAMPORTS_PER_SOL);
      await connection.confirmTransaction(sig, "confirmed");
      console.log(`  Airdrop完了: ${sig}`);
    } catch (e) {
      console.error(`  WARNING: Airdrop失敗: ${e.message}`);
    }
  }

  // TEE暗号化公開鍵の取得
  // Gateway の node-info には含まれないため、TEE /health から取得を試みる
  let teeEncryptionPubkey;
  const envEncPubkey = process.env.TEE_ENCRYPTION_PUBKEY;
  if (envEncPubkey) {
    teeEncryptionPubkey = fromBase64(envEncPubkey);
    console.log(`  TEE encryption_pubkey (env): ${envEncPubkey}`);
  } else {
    // Gateway 経由で TEE の情報を取得
    // /tee-info エンドポイントがなければ、直接 TEE /health にアクセスを試みる
    try {
      // TEE health に encryption_pubkey が含まれる場合
      const teeHealthUrl = GATEWAY_URL.replace(/:3000/, ":4000") + "/health";
      console.log(`  TEE health check: ${teeHealthUrl}`);
      const healthRes = await fetch(teeHealthUrl, { signal: AbortSignal.timeout(5000) });
      if (healthRes.ok) {
        const health = await healthRes.json();
        if (health.encryption_pubkey) {
          teeEncryptionPubkey = fromBase64(health.encryption_pubkey);
          console.log(`  TEE encryption_pubkey (health): ${health.encryption_pubkey}`);
        }
      }
    } catch {
      // TEE port 4000 is not accessible from outside (SG blocks it)
    }

    if (!teeEncryptionPubkey) {
      // tee-info.json がEC2側に保存されているはず
      // ローカルにもあるか確認
      const teeInfoPath = join(PROJECT_ROOT, "tests/e2e/fixtures/tee-info.json");
      if (existsSync(teeInfoPath)) {
        const teeInfo = JSON.parse(readFileSync(teeInfoPath, "utf-8"));
        teeEncryptionPubkey = fromBase64(teeInfo.encryption_pubkey);
        console.log(`  TEE encryption_pubkey (tee-info.json): ${teeInfo.encryption_pubkey}`);
      } else {
        console.error("  ERROR: TEE encryption_pubkey を取得できません。");
        console.error("  以下のいずれかで設定してください:");
        console.error("    1. TEE_ENCRYPTION_PUBKEY 環境変数");
        console.error("    2. tests/e2e/fixtures/tee-info.json");
        console.error("  EC2上で: ssh ec2-user@<IP> cat ~/title-protocol/tests/e2e/fixtures/tee-info.json");
        process.exit(1);
      }
    }
  }

  // フィクスチャ読み込み
  if (!existsSync(FIXTURE_PATH)) {
    console.error(`  ERROR: フィクスチャが見つかりません: ${FIXTURE_PATH}`);
    console.error("  cargo run --example gen_fixture -p title-core -- tests/e2e/fixtures");
    process.exit(1);
  }
  const content = readFileSync(FIXTURE_PATH);
  console.log(`  フィクスチャ: ${FIXTURE_PATH} (${content.length} bytes)`);
  console.log("  OK\n");

  // -----------------------------------------------------------------------
  // Step 1: ClientPayload 構築
  // -----------------------------------------------------------------------
  console.log("[Step 1] ClientPayload 構築...");
  const clientPayload = {
    owner_wallet: wallet.publicKey.toBase58(),
    content: content.toString("base64"),
  };
  const payloadBytes = new TextEncoder().encode(JSON.stringify(clientPayload));
  console.log(`  owner_wallet: ${clientPayload.owner_wallet}`);
  console.log(`  payload size: ${payloadBytes.length} bytes\n`);

  // -----------------------------------------------------------------------
  // Step 2: E2EE暗号化（ECDH + HKDF + AES-GCM）
  // -----------------------------------------------------------------------
  console.log("[Step 2] E2EE暗号化...");
  const ephKeyPair = generateEphemeralKeyPair();
  const sharedSecret = deriveSharedSecret(ephKeyPair.secretKey, teeEncryptionPubkey);
  const symmetricKey = deriveSymmetricKey(sharedSecret);
  const { nonce: encNonce, ciphertext: encCiphertext } = await aesGcmEncrypt(symmetricKey, payloadBytes);

  const encryptedPayload = {
    ephemeral_pubkey: toBase64(ephKeyPair.publicKey),
    nonce: toBase64(encNonce),
    ciphertext: toBase64(encCiphertext),
  };
  console.log(`  ephemeral_pubkey: ${encryptedPayload.ephemeral_pubkey.substring(0, 20)}...`);
  console.log(`  encrypted size: ${encryptedPayload.ciphertext.length} chars (base64)\n`);

  // -----------------------------------------------------------------------
  // Step 3: S3にアップロード（presigned URL経由）
  // -----------------------------------------------------------------------
  console.log("[Step 3] S3にアップロード...");
  const encPayloadJson = JSON.stringify(encryptedPayload);

  const uploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      content_size: encPayloadJson.length,
      content_type: "application/json",
    }),
  });
  if (!uploadUrlRes.ok) {
    const body = await uploadUrlRes.text();
    console.error(`  ERROR: /upload-url 失敗: HTTP ${uploadUrlRes.status} - ${body}`);
    process.exit(1);
  }
  const { upload_url, download_url } = await uploadUrlRes.json();
  console.log(`  download_url: ${download_url.substring(0, 80)}...`);

  const putRes = await fetch(upload_url, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: encPayloadJson,
  });
  if (!putRes.ok) {
    const putBody = await putRes.text();
    console.error(`  ERROR: S3 PUT 失敗: HTTP ${putRes.status}`);
    console.error(`  Response: ${putBody.substring(0, 500)}`);
    console.error(`  Upload URL: ${upload_url.substring(0, 120)}...`);
    process.exit(1);
  }
  console.log("  アップロード完了\n");

  // -----------------------------------------------------------------------
  // Step 4: /verify 呼び出し
  // -----------------------------------------------------------------------
  console.log("[Step 4] /verify 呼び出し...");
  const verifyRes = await fetch(`${GATEWAY_URL}/verify`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      download_url,
      processor_ids: ["core-c2pa"],
    }),
  });
  if (!verifyRes.ok) {
    const body = await verifyRes.text();
    console.error(`  ERROR: /verify 失敗: HTTP ${verifyRes.status} - ${body}`);
    process.exit(1);
  }
  const encryptedResponse = await verifyRes.json();
  console.log(`  暗号化レスポンス受信 (nonce: ${encryptedResponse.nonce.substring(0, 20)}...)\n`);

  // -----------------------------------------------------------------------
  // Step 5: レスポンス復号
  // -----------------------------------------------------------------------
  console.log("[Step 5] レスポンス復号...");
  const decNonce = fromBase64(encryptedResponse.nonce);
  const decCiphertext = fromBase64(encryptedResponse.ciphertext);
  const decryptedBytes = await aesGcmDecrypt(symmetricKey, decNonce, decCiphertext);
  const verifyResponse = JSON.parse(new TextDecoder().decode(decryptedBytes));

  console.log(`  results: ${verifyResponse.results.length} 件`);
  for (const result of verifyResponse.results) {
    console.log(`    processor_id: ${result.processor_id}`);
    const sj = result.signed_json;
    const payload = sj.payload;
    console.log(`    content_hash: ${payload.content_hash}`);
    console.log(`    content_type: ${payload.content_type}`);
    console.log(`    creator_wallet: ${payload.creator_wallet}`);
    console.log(`    tee_pubkey: ${sj.tee_pubkey}`);
    console.log(`    nodes: ${payload.nodes?.length || 0}, links: ${payload.links?.length || 0}`);
  }
  console.log("");

  // -----------------------------------------------------------------------
  // Step 6: signed_json をS3にアップロード
  // -----------------------------------------------------------------------
  console.log("[Step 6] signed_json をS3にアップロード...");
  const signedJson = verifyResponse.results[0].signed_json;
  const signedJsonBytes = new TextEncoder().encode(JSON.stringify(signedJson));

  // 新しいpresigned URLを取得
  const sjUploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      content_size: signedJsonBytes.length,
      content_type: "application/json",
    }),
  });
  if (!sjUploadUrlRes.ok) {
    console.error(`  ERROR: /upload-url (signed_json) 失敗: HTTP ${sjUploadUrlRes.status}`);
    process.exit(1);
  }
  const { upload_url: sjUploadUrl, download_url: signedJsonUri } = await sjUploadUrlRes.json();
  console.log(`  signed_json_uri: ${signedJsonUri.substring(0, 80)}...`);

  const sjPutRes = await fetch(sjUploadUrl, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(signedJson),
  });
  if (!sjPutRes.ok) {
    console.error(`  ERROR: S3 PUT (signed_json) 失敗: HTTP ${sjPutRes.status}`);
    process.exit(1);
  }
  console.log("  アップロード完了\n");

  // -----------------------------------------------------------------------
  // Step 7: /sign 呼び出し
  // -----------------------------------------------------------------------
  console.log("[Step 7] /sign 呼び出し...");
  const { blockhash } = await connection.getLatestBlockhash();
  console.log(`  recent_blockhash: ${blockhash}`);

  const signRes = await fetch(`${GATEWAY_URL}/sign`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      recent_blockhash: blockhash,
      requests: [{ signed_json_uri: signedJsonUri }],
    }),
  });
  if (!signRes.ok) {
    const body = await signRes.text();
    console.error(`  ERROR: /sign 失敗: HTTP ${signRes.status} - ${body}`);
    process.exit(1);
  }
  const signResponse = await signRes.json();
  console.log(`  partial_txs: ${signResponse.partial_txs.length} 件`);
  console.log(`  partial_tx[0] size: ${signResponse.partial_txs[0].length} chars (base64)\n`);

  // -----------------------------------------------------------------------
  // Step 8: クライアントサイドウォレット署名 + ブロードキャスト
  // -----------------------------------------------------------------------
  console.log("[Step 8] ウォレット署名 + ブロードキャスト...");
  const txBytes = Buffer.from(signResponse.partial_txs[0], "base64");
  const tx = Transaction.from(txBytes);

  console.log(`  instructions: ${tx.instructions.length}`);
  console.log(`  signatures (before): ${tx.signatures.length}`);

  // ウォレット署名を追加
  tx.partialSign(wallet);
  console.log("  ウォレット署名完了");

  // ブロードキャスト
  try {
    const rawTx = tx.serialize();
    console.log(`  serialized tx: ${rawTx.length} bytes`);

    const txSig = await connection.sendRawTransaction(rawTx, {
      skipPreflight: false,
      preflightCommitment: "confirmed",
    });
    console.log(`  TX送信完了: ${txSig}`);

    // 確認待ち
    console.log("  確認待ち...");
    const confirmation = await connection.confirmTransaction(txSig, "confirmed");
    if (confirmation.value.err) {
      console.error(`  ERROR: TX確認失敗: ${JSON.stringify(confirmation.value.err)}`);
    } else {
      console.log(`  TX確認完了!`);
      console.log(`  https://explorer.solana.com/tx/${txSig}?cluster=devnet`);
    }
  } catch (e) {
    console.error(`  ERROR: ブロードキャスト失敗: ${e.message}`);
    // ログの詳細
    if (e.logs) {
      console.error("  Logs:");
      for (const log of e.logs) {
        console.error(`    ${log}`);
      }
    }
  }

  // -----------------------------------------------------------------------
  // 結果サマリー
  // -----------------------------------------------------------------------
  console.log("\n=== テスト完了 ===");
  console.log(`  content_hash: ${verifyResponse.results[0].signed_json.payload.content_hash}`);
  console.log(`  creator: ${wallet.publicKey.toBase58()}`);
  console.log(`  Gateway: ${GATEWAY_URL}`);
}

main().catch((e) => {
  console.error("\nFATAL:", e);
  process.exit(1);
});
