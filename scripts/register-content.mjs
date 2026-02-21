#!/usr/bin/env node
/**
 * Title Protocol コンテンツ登録スクリプト（クライアントE2E）
 *
 * ローカル環境からリモートのGateway/TEEにアクセスし、
 * C2PA署名済み画像を仕様書 §6.7 のフローで登録する。
 *
 * 使い方:
 *   GATEWAY_URL=http://<ec2-ip>:3000 \
 *   SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=xxx \
 *   node scripts/register-content.mjs <image.jpg> [--processor core-c2pa,phash-v1]
 *
 * 前提:
 *   - EC2 上で Gateway (:3000) と TEE-mock (:4000) が起動済み
 *   - init-config.mjs で Merkle Tree 作成済み
 *   - tests/e2e/fixtures/tee-info.json が存在（EC2からscpで取得するか、手動作成）
 *   - SOLANA_RPC_URL, GATEWAY_URL 環境変数が設定済み
 */

import { readFileSync, existsSync } from "fs";
import { dirname, join, resolve } from "path";
import { fileURLToPath } from "url";

import {
  Connection,
  Keypair,
  Transaction,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PROJECT_ROOT = dirname(__dirname);

// ---------------------------------------------------------------------------
// 引数パース
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const imagePathArg = args.find((a) => !a.startsWith("--"));
if (!imagePathArg) {
  console.error("Usage: GATEWAY_URL=http://<ec2>:3000 node scripts/register-content.mjs <image.jpg> [--processor core-c2pa,phash-v1]");
  process.exit(1);
}
const IMAGE_PATH = resolve(imagePathArg);

const processorIdx = args.indexOf("--processor");
const PROCESSOR_IDS = processorIdx >= 0 && args[processorIdx + 1]
  ? args[processorIdx + 1].split(",")
  : ["core-c2pa", "phash-v1"];

const SOLANA_RPC = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
const GATEWAY_URL = process.env.GATEWAY_URL;
if (!GATEWAY_URL) {
  console.error("ERROR: GATEWAY_URL 環境変数を設定してください (例: http://<EC2_IP>:3000)");
  process.exit(1);
}

console.log("=== Title Protocol コンテンツ登録 (Client E2E) ===");
console.log(`  Image: ${IMAGE_PATH}`);
console.log(`  Processors: ${PROCESSOR_IDS.join(", ")}`);
console.log(`  Gateway: ${GATEWAY_URL}`);
console.log(`  Solana RPC: ${SOLANA_RPC}`);

// ---------------------------------------------------------------------------
// SDK import (ビルド済み前提)
// ---------------------------------------------------------------------------

const sdkPath = join(PROJECT_ROOT, "sdk", "ts");
const { encryptPayload, decryptResponse } = await import(
  join(sdkPath, "dist", "crypto.js")
);

// ---------------------------------------------------------------------------
// Step 0: Gateway node-info + TEE情報取得
// ---------------------------------------------------------------------------

console.log("\n--- Step 0: ノード情報取得 ---");

const nodeInfoRes = await fetch(`${GATEWAY_URL}/.well-known/title-node-info`);
if (!nodeInfoRes.ok) {
  console.error(`ERROR: Gateway node-info 取得失敗: HTTP ${nodeInfoRes.status}`);
  process.exit(1);
}
const nodeInfo = await nodeInfoRes.json();
console.log(`  Gateway signing_pubkey: ${nodeInfo.signing_pubkey}`);
console.log(`  Supported extensions: ${nodeInfo.supported_extensions?.join(", ")}`);

// tee-info.json からTEE暗号化公開鍵を取得
const teeInfoPath = join(PROJECT_ROOT, "tests", "e2e", "fixtures", "tee-info.json");
if (!existsSync(teeInfoPath)) {
  console.error(`ERROR: tee-info.json が見つかりません: ${teeInfoPath}`);
  console.error("  EC2から scp で取得してください:");
  console.error("  scp ec2:~/title-protocol/tests/e2e/fixtures/tee-info.json tests/e2e/fixtures/");
  process.exit(1);
}
const teeInfo = JSON.parse(readFileSync(teeInfoPath, "utf-8"));
console.log(`  TEE signing_pubkey: ${teeInfo.signing_pubkey}`);
console.log(`  TEE encryption_pubkey: ${teeInfo.encryption_pubkey}`);
console.log(`  Tree address: ${teeInfo.tree_address}`);

// ---------------------------------------------------------------------------
// 画像読み込み
// ---------------------------------------------------------------------------

if (!existsSync(IMAGE_PATH)) {
  console.error(`ERROR: 画像が見つかりません: ${IMAGE_PATH}`);
  process.exit(1);
}
const imageBytes = readFileSync(IMAGE_PATH);
console.log(`  Image size: ${(imageBytes.length / 1024 / 1024).toFixed(2)} MB`);

// ---------------------------------------------------------------------------
// Step 1: ウォレット準備
// ---------------------------------------------------------------------------

console.log("\n--- Step 1: ウォレット準備 ---");
const connection = new Connection(SOLANA_RPC, "confirmed");

const { homedir } = await import("os");
const solanaKeyPath = join(homedir(), ".config", "solana", "id.json");
let wallet;
if (existsSync(solanaKeyPath)) {
  const raw = JSON.parse(readFileSync(solanaKeyPath, "utf-8"));
  wallet = Keypair.fromSecretKey(Uint8Array.from(raw));
  console.log(`  Wallet (from id.json): ${wallet.publicKey.toBase58()}`);
} else {
  wallet = Keypair.generate();
  console.log(`  Wallet (generated): ${wallet.publicKey.toBase58()}`);
  try {
    const sig = await connection.requestAirdrop(wallet.publicKey, 2 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig, "confirmed");
    console.log("  Airdrop: 2 SOL");
  } catch (e) {
    console.log(`  Airdrop skipped: ${e.message?.substring(0, 80)}`);
  }
}

const balance = await connection.getBalance(wallet.publicKey);
console.log(`  Balance: ${balance / LAMPORTS_PER_SOL} SOL`);

// ---------------------------------------------------------------------------
// Step 2: ペイロード構築 + E2EE暗号化 (仕様書 §6.7 Step 2-4)
// ---------------------------------------------------------------------------

console.log("\n--- Step 2: ペイロード暗号化 (ECDH + HKDF + AES-GCM) ---");

const clientPayload = {
  owner_wallet: wallet.publicKey.toBase58(),
  content: Buffer.from(imageBytes).toString("base64"),
};
const payloadBytes = new TextEncoder().encode(JSON.stringify(clientPayload));
console.log(`  ClientPayload size: ${(payloadBytes.length / 1024 / 1024).toFixed(2)} MB`);

const teeEncPubkey = Buffer.from(teeInfo.encryption_pubkey, "base64");
const { symmetricKey, encryptedPayload } = await encryptPayload(teeEncPubkey, payloadBytes);
const encryptedPayloadJson = JSON.stringify(encryptedPayload);
console.log(`  Encrypted size: ${(encryptedPayloadJson.length / 1024 / 1024).toFixed(2)} MB`);

// ---------------------------------------------------------------------------
// Step 3: Temporary Storage にアップロード (仕様書 §6.7 Step 5)
// ---------------------------------------------------------------------------

console.log("\n--- Step 3: Temporary Storage アップロード ---");

const uploadUrlRes = await fetch(`${GATEWAY_URL}/upload-url`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    content_size: encryptedPayloadJson.length,
    content_type: "application/json",
  }),
});
if (!uploadUrlRes.ok) {
  const text = await uploadUrlRes.text();
  console.error(`ERROR: /upload-url failed: HTTP ${uploadUrlRes.status} - ${text}`);
  process.exit(1);
}
const { upload_url, download_url } = await uploadUrlRes.json();
console.log(`  S3 presigned URL 取得: OK`);

const putRes = await fetch(upload_url, {
  method: "PUT",
  headers: { "Content-Type": "application/json" },
  body: encryptedPayloadJson,
});
if (!putRes.ok) {
  console.error(`ERROR: S3 PUT failed: HTTP ${putRes.status}`);
  process.exit(1);
}
console.log("  S3 upload: OK");
console.log(`  download_url: ${download_url}`);

// ---------------------------------------------------------------------------
// Step 4: /verify 呼び出し (仕様書 §6.7 Step 6)
// ---------------------------------------------------------------------------

console.log(`\n--- Step 4: /verify (${PROCESSOR_IDS.join(", ")}) ---`);
console.log("  Processing... (C2PA検証 + WASM実行、数十秒かかります)");

const verifyRes = await fetch(`${GATEWAY_URL}/verify`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    download_url,
    processor_ids: PROCESSOR_IDS,
  }),
});
if (!verifyRes.ok) {
  const text = await verifyRes.text();
  console.error(`ERROR: /verify failed: HTTP ${verifyRes.status} - ${text}`);
  process.exit(1);
}
console.log("  /verify: OK");

// ---------------------------------------------------------------------------
// Step 5: レスポンス復号 (仕様書 §6.7 Step 7)
// ---------------------------------------------------------------------------

console.log("\n--- Step 5: レスポンス復号 ---");

const encryptedResponse = await verifyRes.json();
const decryptedBytes = await decryptResponse(
  symmetricKey,
  encryptedResponse.nonce,
  encryptedResponse.ciphertext
);
const verifyResponse = JSON.parse(new TextDecoder().decode(decryptedBytes));

console.log(`  Results: ${verifyResponse.results.length} processor(s)`);
for (const r of verifyResponse.results) {
  console.log(`    - ${r.processor_id}`);
  const sj = r.signed_json;
  if (sj.payload?.content_hash) {
    console.log(`      content_hash: ${sj.payload.content_hash}`);
  }
  if (sj.payload?.content_type) {
    console.log(`      content_type: ${sj.payload.content_type}`);
  }
  if (sj.payload?.creator_wallet) {
    console.log(`      creator_wallet: ${sj.payload.creator_wallet}`);
  }
  if (sj.payload?.nodes) {
    console.log(`      provenance_nodes: ${sj.payload.nodes.length}`);
  }
  if (sj.payload?.tsa_timestamp) {
    console.log(`      tsa_timestamp: ${sj.payload.tsa_timestamp}`);
  }
  if (sj.payload?.result) {
    const resultStr = JSON.stringify(sj.payload.result);
    console.log(`      wasm_result: ${resultStr.substring(0, 200)}${resultStr.length > 200 ? "..." : ""}`);
  }
  if (sj.payload?.extension_id) {
    console.log(`      extension_id: ${sj.payload.extension_id}`);
    console.log(`      wasm_hash: ${sj.payload.wasm_hash}`);
  }
}

// ---------------------------------------------------------------------------
// Step 6: signed_json をオフチェーンストレージに保存 (仕様書 §6.7 Step 9)
//
// 本番ではArweave (Irys)。ここではS3 presigned URLを流用する。
// TEE (PROXY_ADDR=direct) が同じS3から直接GETできるため。
// ---------------------------------------------------------------------------

console.log("\n--- Step 6: signed_json → S3 保存 ---");

const signedJsonUris = [];
for (const result of verifyResponse.results) {
  const jsonStr = JSON.stringify(result.signed_json);
  const jsonBytes = new TextEncoder().encode(jsonStr);

  // Gateway /upload-url で新しいpresigned URLを取得
  const sjUploadRes = await fetch(`${GATEWAY_URL}/upload-url`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      content_size: jsonBytes.length,
      content_type: "application/json",
    }),
  });
  if (!sjUploadRes.ok) {
    console.error(`ERROR: signed_json upload-url failed: HTTP ${sjUploadRes.status}`);
    process.exit(1);
  }
  const sjUrls = await sjUploadRes.json();

  // S3にPUT
  const sjPutRes = await fetch(sjUrls.upload_url, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: jsonStr,
  });
  if (!sjPutRes.ok) {
    console.error(`ERROR: signed_json S3 PUT failed: HTTP ${sjPutRes.status}`);
    process.exit(1);
  }

  signedJsonUris.push(sjUrls.download_url.split("?")[0]);
  console.log(`  ${result.processor_id}: stored (${jsonBytes.length} bytes)`);
}

// ---------------------------------------------------------------------------
// Step 7: /sign 呼び出し (仕様書 §6.7 Step 10)
// ---------------------------------------------------------------------------

console.log("\n--- Step 7: /sign ---");

const { blockhash } = await connection.getLatestBlockhash();
console.log(`  Recent blockhash: ${blockhash}`);

const signRes = await fetch(`${GATEWAY_URL}/sign`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    recent_blockhash: blockhash,
    requests: signedJsonUris.map((uri) => ({ signed_json_uri: uri })),
  }),
});
if (!signRes.ok) {
  const text = await signRes.text();
  console.error(`ERROR: /sign failed: HTTP ${signRes.status} - ${text}`);
  process.exit(1);
}

const signResponse = await signRes.json();
console.log(`  Partial TXs: ${signResponse.partial_txs.length}`);

// ---------------------------------------------------------------------------
// Step 8: ウォレット署名 + ブロードキャスト (仕様書 §6.7 Step 11)
// ---------------------------------------------------------------------------

console.log("\n--- Step 8: ウォレット署名 + Solanaブロードキャスト ---");

const txSignatures = [];
for (let i = 0; i < signResponse.partial_txs.length; i++) {
  const txBytes = Buffer.from(signResponse.partial_txs[i], "base64");
  const tx = Transaction.from(txBytes);

  // 信頼TEEノードの署名確認
  const hasTrustedSigner = tx.signatures.some(
    (sig) => sig.publicKey && sig.publicKey.toBase58() === teeInfo.signing_pubkey
  );
  if (!hasTrustedSigner) {
    console.error(`  TX ${i + 1}: ERROR - TEE署名が見つかりません`);
    continue;
  }
  console.log(`  TX ${i + 1}: TEE署名確認OK`);

  // ウォレットで署名
  tx.partialSign(wallet);

  try {
    const sig = await connection.sendRawTransaction(tx.serialize(), {
      skipPreflight: true,
    });
    console.log(`  TX ${i + 1}: ${sig}`);
    console.log(`    → https://explorer.solana.com/tx/${sig}?cluster=devnet`);

    await connection.confirmTransaction(sig, "confirmed");
    console.log(`    ✓ Confirmed`);
    txSignatures.push(sig);
  } catch (e) {
    console.error(`  TX ${i + 1} failed: ${e.message?.substring(0, 200)}`);
  }
}

// ---------------------------------------------------------------------------
// 結果サマリ
// ---------------------------------------------------------------------------

console.log("\n========================================");
console.log("  登録完了");
console.log("========================================");
console.log(`  Image: ${IMAGE_PATH}`);
console.log(`  Owner: ${wallet.publicKey.toBase58()}`);
console.log(`  Processors: ${PROCESSOR_IDS.join(", ")}`);
console.log(`  Tree: ${teeInfo.tree_address}`);
console.log(`  TXs: ${txSignatures.length}`);
for (const sig of txSignatures) {
  console.log(`    https://explorer.solana.com/tx/${sig}?cluster=devnet`);
}
if (txSignatures.length === 0) {
  console.log("  WARNING: トランザクションがブロードキャストされていません");
}

process.exit(0);
