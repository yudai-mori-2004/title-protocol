#!/usr/bin/env tsx
// SPDX-License-Identifier: Apache-2.0

/**
 * test-phash-similarity.ts — pHash類似度テスト
 *
 * 同じラーメン画像の編集バリエーション（リサイズ、クロップ、低画質圧縮）を
 * phash-v1 Extension付きで登録し、以下を検証する:
 *
 * 1. 全バリエーションのpHashがオリジナルと十分に近い（ハミング距離 ≤ 閾値）
 * 2. DASでcontent_hash検索し、TSAタイムスタンプからオリジナルを特定できる
 *
 * Usage:
 *   npx tsx test-phash-similarity.ts <gateway-ip> --wallet <keypair.json> [options]
 *
 * Examples:
 *   npx tsx test-phash-similarity.ts 54.250.143.52 --wallet ../keys/operator.json --skip-sign
 *   npx tsx test-phash-similarity.ts 54.250.143.52 --wallet ../keys/operator.json --broadcast
 */

import { webcrypto } from "node:crypto";
if (!globalThis.crypto?.subtle) {
  // @ts-ignore
  globalThis.crypto = webcrypto;
}

import * as fs from "node:fs";
import * as path from "node:path";

import {
  Connection,
  Keypair,
  Transaction,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import bs58 from "bs58";
import {
  TitleClient,
  type GlobalConfig,
  type VerifyResponse,
  encryptPayload,
  decryptResponse,
  fetchGlobalConfig,
} from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// 定数
// ---------------------------------------------------------------------------

const FIXTURES_DIR = path.resolve(
  path.dirname(new URL(import.meta.url).pathname),
  "fixtures/phash-test/signed",
);
const HAMMING_THRESHOLD = 10; // 64bitハッシュで10bit以内 = 84%以上一致
// 構図変更（大幅クロップ）は閾値を超える — これは正しい挙動。
// pHashは「同じ構図の軽微な編集」を検出するもので、構図自体の変更は別コンテンツと判定される。
const CROP_DISTANCE_MIN = 15; // 大幅クロップは閾値を大きく超えることを確認

// ---------------------------------------------------------------------------
// CLI引数
// ---------------------------------------------------------------------------

interface Args {
  gatewayHost: string;
  port: number;
  solanaRpc: string;
  walletPath: string;
  skipSign: boolean;
  broadcast: boolean;
}

function parseArgs(): Args {
  const args = process.argv.slice(2);
  if (args.length < 1 || args[0] === "--help") {
    console.log(`
Usage: npx tsx test-phash-similarity.ts <gateway-ip> --wallet <keypair.json> [options]

Options:
  --port <n>       Gateway port (default: 3000)
  --rpc <url>      Solana RPC URL
  --skip-sign      Stop after /verify
  --broadcast      Co-sign and broadcast cNFT mint tx
`);
    process.exit(0);
  }

  const gatewayHost = args[0];
  let port = 3000;
  let solanaRpc = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
  let walletPath = "";
  let skipSign = false;
  let broadcast = false;

  for (let i = 1; i < args.length; i++) {
    switch (args[i]) {
      case "--port": port = parseInt(args[++i], 10); break;
      case "--rpc": solanaRpc = args[++i]; break;
      case "--wallet": walletPath = args[++i]; break;
      case "--skip-sign": skipSign = true; break;
      case "--broadcast": broadcast = true; break;
    }
  }

  if (!walletPath) {
    console.error("エラー: --wallet <keypair.json> は必須です");
    process.exit(1);
  }

  return { gatewayHost, port, solanaRpc, walletPath, skipSign, broadcast };
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

function log(label: string, ...msg: unknown[]) {
  const ts = new Date().toISOString().slice(11, 23);
  console.log(`[${ts}] ${label}`, ...msg);
}

function loadKeypair(p: string): Keypair {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(p, "utf-8"))));
}

function hammingDistance(a: string, b: string): number {
  const va = BigInt("0x" + a);
  const vb = BigInt("0x" + b);
  let xor = va ^ vb;
  let count = 0;
  while (xor > 0n) {
    count += Number(xor & 1n);
    xor >>= 1n;
  }
  return count;
}

// ---------------------------------------------------------------------------
// 画像登録（verify + 任意でsign/broadcast）
// ---------------------------------------------------------------------------

interface RegisterResult {
  filename: string;
  contentHash: string;
  phash: string;
  tsaTimestamp?: number;
  arweaveUrls?: string[];
  txSig?: string;
}

async function registerImage(
  client: TitleClient,
  session: Awaited<ReturnType<TitleClient["selectNode"]>>,
  connection: Connection,
  keypair: Keypair,
  imagePath: string,
  args: Args,
): Promise<RegisterResult> {
  const filename = path.basename(imagePath);
  const imageBytes = fs.readFileSync(imagePath);
  log("REG", `${filename} (${(imageBytes.length / 1024).toFixed(0)} KB)`);

  // 暗号化 + アップロード
  const contentB64 = Buffer.from(imageBytes).toString("base64");
  const payloadJson = new TextEncoder().encode(JSON.stringify({
    owner_wallet: keypair.publicKey.toBase58(),
    content: contentB64,
  }));

  const teeEncPubkeyBytes = Buffer.from(session.encryptionPubkey, "base64");
  const { symmetricKey, encryptedPayload } = await encryptPayload(
    new Uint8Array(teeEncPubkeyBytes),
    payloadJson,
  );

  const { downloadUrl } = await client.upload(session.gatewayUrl, encryptedPayload);

  // /verify (core-c2pa + phash-v1)
  const t0 = Date.now();
  const encryptedResponse = await client.verifyRaw(session.gatewayUrl, {
    download_url: downloadUrl,
    processor_ids: ["core-c2pa", "phash-v1"],
  });

  const responsePlaintext = await decryptResponse(
    symmetricKey,
    encryptedResponse.nonce,
    encryptedResponse.ciphertext,
  );
  const verifyResponse: VerifyResponse = JSON.parse(
    new TextDecoder().decode(responsePlaintext),
  );
  log("REG", `  /verify ${Date.now() - t0}ms — ${verifyResponse.results.length} processor(s)`);

  // 結果抽出
  const coreResult = verifyResponse.results.find((r) => r.processor_id === "core-c2pa");
  const phashResult = verifyResponse.results.find((r) => r.processor_id === "phash-v1");

  const coreSj = coreResult?.signed_json as any;
  const phashSj = phashResult?.signed_json as any;

  const contentHash = coreSj?.payload?.content_hash ?? "unknown";
  const phash = phashSj?.payload?.phash ?? "unknown";
  const tsaTimestamp = coreSj?.payload?.tsa_timestamp;

  log("REG", `  content_hash: ${contentHash}`);
  log("REG", `  phash:        ${phash}`);
  if (tsaTimestamp) {
    log("REG", `  tsa_timestamp: ${tsaTimestamp} (${new Date(tsaTimestamp * 1000).toISOString()})`);
  }

  const result: RegisterResult = { filename, contentHash, phash, tsaTimestamp };

  // sign + broadcast
  if (!args.skipSign) {
    // Arweave upload
    const { Uploader } = await import("@irys/upload");
    const { Solana } = await import("@irys/upload-solana");
    const irys = await Uploader(Solana)
      .withWallet(bs58.encode(keypair.secretKey))
      .withRpc(args.solanaRpc)
      .devnet()
      .build();

    const arweaveUrls: string[] = [];
    const signRequests: { signed_json_uri: string }[] = [];
    for (const r of verifyResponse.results) {
      const jsonStr = JSON.stringify(r.signed_json);
      const tags = [{ name: "Content-Type", value: "application/json" }];
      const receipt = await irys.upload(jsonStr, { tags });
      const url = `https://gateway.irys.xyz/${receipt.id}`;
      arweaveUrls.push(url);
      signRequests.push({ signed_json_uri: url });
    }
    result.arweaveUrls = arweaveUrls;

    // /sign
    const { blockhash } = await connection.getLatestBlockhash();
    const signResponse = await client.signRaw(session.gatewayUrl, {
      recent_blockhash: blockhash,
      requests: signRequests,
    });

    if (args.broadcast) {
      for (const ptx of signResponse.partial_txs) {
        const tx = Transaction.from(Buffer.from(ptx, "base64"));
        tx.partialSign(keypair);
        const sig = await connection.sendRawTransaction(tx.serialize(), {
          skipPreflight: false,
          preflightCommitment: "confirmed",
        });
        await connection.confirmTransaction(sig, "confirmed");
        result.txSig = sig;
        log("REG", `  cNFT minted: ${sig}`);
      }
    }
  }

  return result;
}

// ---------------------------------------------------------------------------
// メイン
// ---------------------------------------------------------------------------

async function main() {
  const args = parseArgs();
  const connection = new Connection(args.solanaRpc, "confirmed");
  const keypair = loadKeypair(args.walletPath);

  log("TEST", "=== pHash類似度テスト ===");
  log("TEST", `Gateway: ${args.gatewayHost}:${args.port}`);
  log("TEST", `Wallet:  ${keypair.publicKey.toBase58()}`);

  // フィクスチャ確認
  if (!fs.existsSync(FIXTURES_DIR)) {
    console.error(`フィクスチャが見つかりません: ${FIXTURES_DIR}`);
    console.error("先に gen_phash_fixtures を実行してください:");
    console.error("  cargo run --example gen_phash_fixtures -- integration-tests/fixtures/phash-test --tsa http://timestamp.digicert.com");
    process.exit(1);
  }

  const imageFiles = fs.readdirSync(FIXTURES_DIR)
    .filter((f) => f.endsWith(".jpg") || f.endsWith(".png"))
    .sort((a, b) => {
      // original を最初に
      if (a.includes("original")) return -1;
      if (b.includes("original")) return 1;
      return a.localeCompare(b);
    });

  log("TEST", `フィクスチャ: ${imageFiles.length} 枚`);
  for (const f of imageFiles) {
    const stat = fs.statSync(path.join(FIXTURES_DIR, f));
    log("TEST", `  ${f} (${(stat.size / 1024).toFixed(0)} KB)`);
  }

  // GlobalConfig取得
  log("TEST", "GlobalConfig を取得中...");
  const globalConfig = await fetchGlobalConfig(connection);
  const client = new TitleClient(globalConfig);
  const session = await client.selectNode();
  log("TEST", `ノード: ${session.signingPubkey}`);

  // 全画像を登録
  log("TEST", "\n--- 登録フェーズ ---");
  const results: RegisterResult[] = [];
  for (const file of imageFiles) {
    const result = await registerImage(
      client, session, connection, keypair,
      path.join(FIXTURES_DIR, file),
      args,
    );
    results.push(result);
    console.log();
  }

  // ---------------------------------------------------------------------------
  // 検証フェーズ
  // ---------------------------------------------------------------------------
  log("TEST", "\n--- 検証フェーズ ---");

  const original = results.find((r) => r.filename.includes("original"));
  if (!original) {
    console.error("ERROR: original が見つかりません");
    process.exit(1);
  }

  log("TEST", `オリジナル: ${original.filename}`);
  log("TEST", `  phash: ${original.phash}`);
  log("TEST", `  content_hash: ${original.contentHash}`);

  // 1. ハミング距離テスト
  log("TEST", `\n[1] ハミング距離テスト (類似閾値: ≤ ${HAMMING_THRESHOLD} bits)`);
  let allPass = true;
  for (const r of results) {
    const dist = hammingDistance(original.phash, r.phash);
    const isCrop = r.filename.includes("crop");

    let pass: boolean;
    let expected: string;
    if (isCrop) {
      // 大幅クロップは構図変更 → 閾値を超えることを期待
      pass = dist >= CROP_DISTANCE_MIN;
      expected = `≥ ${CROP_DISTANCE_MIN} (構図変更)`;
    } else {
      // リサイズ・再圧縮等 → 閾値以内を期待
      pass = dist <= HAMMING_THRESHOLD;
      expected = `≤ ${HAMMING_THRESHOLD} (類似)`;
    }

    const status = pass ? "PASS" : "FAIL";
    log("TEST", `  ${status} ${r.filename}: distance=${dist}/64  (expected: ${expected})`);
    if (!pass) allPass = false;
  }

  // 2. TSAタイムスタンプ検証（originalが最古）
  log("TEST", "\n[2] TSAタイムスタンプ順序テスト");
  const withTsa = results.filter((r) => r.tsaTimestamp != null);
  if (withTsa.length > 0) {
    const sorted = [...withTsa].sort((a, b) => (a.tsaTimestamp ?? 0) - (b.tsaTimestamp ?? 0));
    const oldestIsOriginal = sorted[0].filename.includes("original");
    log("TEST", `  最古の画像: ${sorted[0].filename} (TSA: ${sorted[0].tsaTimestamp})`);
    log("TEST", `  最新の画像: ${sorted[sorted.length - 1].filename} (TSA: ${sorted[sorted.length - 1].tsaTimestamp})`);
    log("TEST", `  originalが最古: ${oldestIsOriginal ? "PASS" : "FAIL"}`);
    if (!oldestIsOriginal) allPass = false;
  } else {
    log("TEST", "  TSAタイムスタンプなし（スキップ）");
  }

  // 3. 類似グループからオリジナル特定
  log("TEST", "\n[3] 類似コンテンツからオリジナル特定テスト");
  const similarGroup = results.filter((r) => {
    const dist = hammingDistance(original.phash, r.phash);
    return dist <= HAMMING_THRESHOLD;
  });
  log("TEST", `  phash類似グループ: ${similarGroup.length}/${results.length} 枚`);

  if (withTsa.length > 0) {
    const similarWithTsa = similarGroup.filter((r) => r.tsaTimestamp != null);
    const earliest = similarWithTsa.reduce((a, b) =>
      (a.tsaTimestamp ?? Infinity) < (b.tsaTimestamp ?? Infinity) ? a : b
    );
    const foundOriginal = earliest.filename.includes("original");
    log("TEST", `  TSA最古 → ${earliest.filename}: ${foundOriginal ? "PASS (オリジナル特定成功)" : "FAIL"}`);
    if (!foundOriginal) allPass = false;
  }

  // 結果サマリ
  log("TEST", "\n=== 結果 ===");
  log("TEST", allPass ? "ALL TESTS PASSED" : "SOME TESTS FAILED");

  process.exit(allPass ? 0 : 1);
}

main().catch((e) => {
  console.error("Fatal:", e);
  process.exit(1);
});
