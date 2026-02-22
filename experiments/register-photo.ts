#!/usr/bin/env tsx
/**
 * register-photo.ts — 実画像をSDK経由でregisterする実験スクリプト
 *
 * signed_jsonはIrys経由でArweave devnetに保存。
 * ユーザーwalletがIrysアップロード費用・Solanaガス代を全て負担する。
 *
 * Usage:
 *   npx tsx register-photo.ts <gateway-ip> <image-path> --wallet <keypair.json> [options]
 *
 * Examples:
 *   # verify + sign (Arweave保存 → cNFT発行トランザクション取得)
 *   npx tsx register-photo.ts 54.238.1.100 ../--help/pixel_photo_ramen.jpg \
 *     --wallet ~/.config/solana/id.json
 *
 *   # verify + sign + broadcast (実際にオンチェーン発行)
 *   npx tsx register-photo.ts 54.238.1.100 ../--help/pixel_photo_ramen.jpg \
 *     --wallet ~/.config/solana/id.json --broadcast
 *
 *   # verify のみ (sign スキップ)
 *   npx tsx register-photo.ts 54.238.1.100 ../--help/pixel_photo_ramen.jpg \
 *     --wallet ~/.config/solana/id.json --skip-sign
 *
 *   # 追加オプション
 *   npx tsx register-photo.ts 54.238.1.100 ../--help/pixel_photo_ramen.jpg \
 *     --wallet ~/.config/solana/id.json \
 *     --port 3000 \
 *     --rpc https://api.devnet.solana.com \
 *     --processors core-c2pa,phash-v1
 */

import * as fs from "node:fs";
import * as path from "node:path";

import {
  Connection,
  Keypair,
  Transaction,
  LAMPORTS_PER_SOL,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import bs58 from "bs58";
import { Uploader } from "@irys/upload";
import { Solana } from "@irys/upload-solana";
import {
  TitleClient,
  type TitleClientConfig,
  type GlobalConfig,
  type TrustedTeeNode,
  type NodeInfo,
  type VerifyResponse,
  encryptPayload,
  decryptResponse,
} from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// CLI引数パース
// ---------------------------------------------------------------------------

interface Args {
  gatewayHost: string;
  imagePath: string;
  port: number;
  solanaRpc: string;
  walletPath: string;
  skipSign: boolean;
  broadcast: boolean;
  processorIds: string[];
}

function parseArgs(): Args {
  const args = process.argv.slice(2);

  if (args.length < 2 || args[0] === "--help" || args[0] === "-h") {
    console.log(`
Usage: npx tsx register-photo.ts <gateway-ip> <image-path> --wallet <keypair.json> [options]

Arguments:
  gateway-ip    Gateway server IP or hostname
  image-path    Path to image file (JPEG with C2PA metadata)

Required:
  --wallet <path>      Solana keypair JSON file (pays for Irys + Solana gas)

Options:
  --port <n>           Gateway port (default: 3000)
  --rpc <url>          Solana RPC URL (default: env SOLANA_RPC_URL or devnet)
  --skip-sign          Stop after /verify, don't call /sign
  --broadcast          After /sign, co-sign and broadcast tx to Solana
  --processors <ids>   Comma-separated processor IDs (default: core-c2pa)
`);
    process.exit(0);
  }

  const gatewayHost = args[0];
  const imagePath = args[1];

  let port = 3000;
  let solanaRpc =
    process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
  let walletPath: string | null = null;
  let skipSign = false;
  let broadcast = false;
  let processorIds = ["core-c2pa"];

  for (let i = 2; i < args.length; i++) {
    switch (args[i]) {
      case "--port":
        port = parseInt(args[++i], 10);
        break;
      case "--rpc":
        solanaRpc = args[++i];
        break;
      case "--wallet":
        walletPath = args[++i];
        break;
      case "--skip-sign":
        skipSign = true;
        break;
      case "--broadcast":
        broadcast = true;
        break;
      case "--processors":
        processorIds = args[++i].split(",");
        break;
    }
  }

  if (!walletPath) {
    console.error("エラー: --wallet <keypair.json> は必須です");
    process.exit(1);
  }

  return {
    gatewayHost,
    imagePath,
    port,
    solanaRpc,
    walletPath,
    skipSign,
    broadcast,
    processorIds,
  };
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

function log(label: string, ...msg: unknown[]) {
  const ts = new Date().toISOString().slice(11, 23);
  console.log(`[${ts}] ${label}`, ...msg);
}

function loadKeypair(walletPath: string): Keypair {
  const raw = JSON.parse(fs.readFileSync(walletPath, "utf-8"));
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

/**
 * Irys uploader を初期化する（Solana devnet）。
 * ユーザーwalletの秘密鍵で署名する。
 */
async function createIrysUploader(keypair: Keypair, rpcUrl: string) {
  const secretKeyBs58 = bs58.encode(keypair.secretKey);
  const irys = await Uploader(Solana)
    .withWallet(secretKeyBs58)
    .withRpc(rpcUrl)
    .devnet()
    .build();
  return irys;
}

/**
 * Irysにデータをアップロードし、gateway URLを返す。
 * 残高不足の場合は自動でfundする。
 */
async function uploadToIrys(
  irys: Awaited<ReturnType<typeof createIrysUploader>>,
  data: string,
  contentType: string
): Promise<string> {
  const size = Buffer.byteLength(data);

  // 費用確認
  const price = await irys.getPrice(size);
  const balance = await irys.getBalance();

  if (balance.isLessThan(price)) {
    const deficit = price.minus(balance);
    log("IRYS", `残高不足: ${irys.utils.fromAtomic(balance)} SOL < ${irys.utils.fromAtomic(price)} SOL`);
    log("IRYS", `${irys.utils.fromAtomic(deficit)} SOL をfundします...`);
    // 少し余裕を持ってfund (2倍)
    await irys.fund(price.multipliedBy(2));
    log("IRYS", "fund完了");
  }

  const tags = [{ name: "Content-Type", value: contentType }];
  const receipt = await irys.upload(data, { tags });
  return `https://gateway.irys.xyz/${receipt.id}`;
}

// ---------------------------------------------------------------------------
// メイン
// ---------------------------------------------------------------------------

async function main() {
  const args = parseArgs();
  const gatewayUrl = `http://${args.gatewayHost}:${args.port}`;

  // 画像読み込み
  const absPath = path.resolve(args.imagePath);
  if (!fs.existsSync(absPath)) {
    console.error(`ファイルが見つかりません: ${absPath}`);
    process.exit(1);
  }
  const imageBytes = fs.readFileSync(absPath);
  log("FILE", `${path.basename(absPath)} (${(imageBytes.length / 1024).toFixed(1)} KB)`);

  // ウォレット
  const keypair = loadKeypair(args.walletPath);
  log("WALLET", keypair.publicKey.toBase58());

  // 残高確認
  const connection = new Connection(args.solanaRpc, "confirmed");
  const balance = await connection.getBalance(keypair.publicKey);
  log("WALLET", `残高: ${(balance / LAMPORTS_PER_SOL).toFixed(4)} SOL`);
  if (balance < 0.01 * LAMPORTS_PER_SOL) {
    console.error("警告: SOL残高が少なすぎます。devnetの場合: solana airdrop 2 --url devnet");
  }

  // ---------------------------------------------------------------------------
  // Step 1: Gateway node-info 取得
  // ---------------------------------------------------------------------------
  log("STEP 1", "node-info を取得中...");
  const nodeInfoRes = await fetch(
    `${gatewayUrl}/.well-known/title-node-info`
  );
  if (!nodeInfoRes.ok) {
    console.error(
      `Gateway接続失敗: HTTP ${nodeInfoRes.status} ${await nodeInfoRes.text()}`
    );
    process.exit(1);
  }
  const nodeInfo = (await nodeInfoRes.json()) as NodeInfo;
  log("STEP 1", `signing_pubkey: ${nodeInfo.signing_pubkey}`);
  log("STEP 1", `extensions: [${nodeInfo.supported_extensions.join(", ")}]`);
  log(
    "STEP 1",
    `limits: max_single=${(nodeInfo.limits.max_single_content_bytes / 1024 / 1024).toFixed(1)}MB`
  );

  // ---------------------------------------------------------------------------
  // Step 2: TEE暗号化公開鍵を取得
  // ---------------------------------------------------------------------------
  log("STEP 2", "TEE encryption_pubkey を取得中...");
  const teeDirectUrl = `http://${args.gatewayHost}:4000`;
  let encryptionPubkey: string;
  try {
    const teeInfoRes = await fetch(
      `${teeDirectUrl}/.well-known/title-node-info`
    );
    if (teeInfoRes.ok) {
      const teeInfo = (await teeInfoRes.json()) as any;
      encryptionPubkey = teeInfo.encryption_pubkey;
      log(
        "STEP 2",
        `encryption_pubkey: ${encryptionPubkey.slice(0, 20)}...`
      );
    } else {
      throw new Error(`TEE直接アクセス失敗: HTTP ${teeInfoRes.status}`);
    }
  } catch (e: any) {
    console.error(`TEEからencryption_pubkey取得に失敗: ${e.message}`);
    console.error(
      "  TEEが直接アクセス可能であることを確認してください (port 4000)"
    );
    process.exit(1);
  }

  // ---------------------------------------------------------------------------
  // TitleClient 構築
  // ---------------------------------------------------------------------------
  const teeNode: TrustedTeeNode = {
    signing_pubkey: nodeInfo.signing_pubkey,
    encryption_pubkey: encryptionPubkey,
    encryption_algorithm: "x25519-hkdf-sha256-aes256gcm",
    gateway_pubkey: nodeInfo.signing_pubkey,
    gateway_endpoint: gatewayUrl,
    status: "active",
    tee_type: "mock",
    expected_measurements: {},
  };

  const globalConfig: GlobalConfig = {
    authority: "",
    core_collection_mint: "",
    ext_collection_mint: "",
    trusted_tee_nodes: [teeNode],
    trusted_tsa_keys: [],
    trusted_wasm_modules: [],
  };

  const client = new TitleClient({
    teeNodes: [gatewayUrl],
    solanaRpcUrl: args.solanaRpc,
    globalConfig,
  });

  // ---------------------------------------------------------------------------
  // Step 3: ペイロード暗号化 + アップロード
  // ---------------------------------------------------------------------------
  log("STEP 3", "ペイロードを暗号化中...");

  const contentB64 = Buffer.from(imageBytes).toString("base64");
  const clientPayload = {
    owner_wallet: keypair.publicKey.toBase58(),
    content: contentB64,
  };
  const payloadJson = new TextEncoder().encode(
    JSON.stringify(clientPayload)
  );

  const teeEncPubkeyBytes = Buffer.from(encryptionPubkey, "base64");
  const { symmetricKey, encryptedPayload } = await encryptPayload(
    new Uint8Array(teeEncPubkeyBytes),
    payloadJson
  );
  log(
    "STEP 3",
    `暗号化完了 (ciphertext: ${(encryptedPayload.ciphertext.length / 1024).toFixed(1)} KB base64)`
  );

  log("STEP 3", "Temporary Storageにアップロード中...");
  const { downloadUrl, sizeBytes } = await client.upload(
    gatewayUrl,
    encryptedPayload
  );
  log(
    "STEP 3",
    `アップロード完了 (${(sizeBytes / 1024).toFixed(1)} KB) → ${downloadUrl}`
  );

  // ---------------------------------------------------------------------------
  // Step 4: /verify
  // ---------------------------------------------------------------------------
  log(
    "STEP 4",
    `/verify を呼び出し中... (processors: [${args.processorIds.join(", ")}])`
  );
  const t0 = Date.now();

  const encryptedResponse = await client.verify(gatewayUrl, {
    download_url: downloadUrl,
    processor_ids: args.processorIds,
  });

  const verifyMs = Date.now() - t0;
  log("STEP 4", `/verify 完了 (${verifyMs}ms)`);

  // レスポンス復号
  const responsePlaintext = await decryptResponse(
    symmetricKey,
    encryptedResponse.nonce,
    encryptedResponse.ciphertext
  );
  const verifyResponse: VerifyResponse = JSON.parse(
    new TextDecoder().decode(responsePlaintext)
  );

  log("STEP 4", `結果: ${verifyResponse.results.length} processor(s)`);

  for (const result of verifyResponse.results) {
    const sj = result.signed_json as any;
    console.log(`\n  --- ${result.processor_id} ---`);
    console.log(`  protocol:       ${sj.protocol}`);
    console.log(`  tee_type:       ${sj.tee_type}`);
    console.log(`  tee_pubkey:     ${sj.tee_pubkey}`);
    if (sj.payload?.content_hash) {
      console.log(`  content_hash:   ${sj.payload.content_hash}`);
    }
    if (sj.payload?.content_type) {
      console.log(`  content_type:   ${sj.payload.content_type}`);
    }
    if (sj.payload?.creator_wallet) {
      console.log(`  creator_wallet: ${sj.payload.creator_wallet}`);
    }
    if (sj.payload?.nodes) {
      console.log(`  nodes:          ${sj.payload.nodes.length} node(s)`);
      for (const n of sj.payload.nodes) {
        console.log(`    - ${n.id} (${n.node_type})`);
      }
    }
    if (sj.payload?.links?.length > 0) {
      console.log(`  links:          ${sj.payload.links.length} link(s)`);
    }
    if (sj.attributes) {
      console.log(`  attributes:`);
      for (const a of sj.attributes) {
        const val =
          a.value.length > 60 ? a.value.slice(0, 60) + "..." : a.value;
        console.log(`    ${a.trait_type}: ${val}`);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Step 5: signed_json を Arweave devnet にアップロード (via Irys)
  // ---------------------------------------------------------------------------
  if (args.skipSign) {
    log("DONE", "--skip-sign 指定のため /sign をスキップします");
    const outPath = path.resolve("output-verify.json");
    fs.writeFileSync(outPath, JSON.stringify(verifyResponse, null, 2));
    log("DONE", `verify結果を保存: ${outPath}`);
    process.exit(0);
  }

  log("STEP 5", "Irys uploader を初期化中 (Solana devnet)...");
  const irys = await createIrysUploader(keypair, args.solanaRpc);
  log("STEP 5", `Irys address: ${irys.address}`);

  const irysBalance = await irys.getBalance();
  log("STEP 5", `Irys残高: ${irys.utils.fromAtomic(irysBalance)} SOL`);

  const signRequests: { signed_json_uri: string }[] = [];
  for (const result of verifyResponse.results) {
    const jsonStr = JSON.stringify(result.signed_json);
    log(
      "STEP 5",
      `${result.processor_id} をArweaveにアップロード中 (${(Buffer.byteLength(jsonStr) / 1024).toFixed(1)} KB)...`
    );

    const arweaveUrl = await uploadToIrys(
      irys,
      jsonStr,
      "application/json"
    );
    signRequests.push({ signed_json_uri: arweaveUrl });
    log("STEP 5", `  → ${arweaveUrl}`);
  }

  // ---------------------------------------------------------------------------
  // Step 6: /sign
  // ---------------------------------------------------------------------------
  const { blockhash, lastValidBlockHeight } =
    await connection.getLatestBlockhash();
  log("STEP 6", `blockhash: ${blockhash}`);

  log("STEP 6", `/sign を呼び出し中...`);
  const t1 = Date.now();

  const signResponse = await client.sign(gatewayUrl, {
    recent_blockhash: blockhash,
    requests: signRequests,
  });

  const signMs = Date.now() - t1;
  log(
    "STEP 6",
    `/sign 完了 (${signMs}ms) — ${signResponse.partial_txs.length} tx(s)`
  );

  for (let i = 0; i < signResponse.partial_txs.length; i++) {
    const txBytes = Buffer.from(signResponse.partial_txs[i], "base64");
    log(
      "STEP 6",
      `  tx[${i}]: ${txBytes.length} bytes`
    );

    try {
      const tx = Transaction.from(txBytes);
      log(
        "STEP 6",
        `  tx[${i}] signers: ${tx.signatures.length}, instructions: ${tx.instructions.length}`
      );
    } catch (e: any) {
      log("STEP 6", `  tx[${i}] デシリアライズ失敗: ${e.message}`);
    }
  }

  // ---------------------------------------------------------------------------
  // Step 7: broadcast (optional)
  // ---------------------------------------------------------------------------
  if (args.broadcast) {
    log("STEP 7", "トランザクションをブロードキャスト中...");

    for (let i = 0; i < signResponse.partial_txs.length; i++) {
      const txBytes = Buffer.from(signResponse.partial_txs[i], "base64");
      const tx = Transaction.from(txBytes);

      // ユーザーwalletで共同署名
      tx.partialSign(keypair);

      log("STEP 7", `  tx[${i}] をSolanaに送信中...`);
      try {
        const sig = await sendAndConfirmTransaction(connection, tx, [keypair], {
          skipPreflight: false,
          commitment: "confirmed",
        });
        log("STEP 7", `  tx[${i}] 確認済み: ${sig}`);
        log("STEP 7", `  https://explorer.solana.com/tx/${sig}?cluster=devnet`);
      } catch (e: any) {
        log("ERROR", `  tx[${i}] ブロードキャスト失敗: ${e.message}`);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // 結果出力
  // ---------------------------------------------------------------------------
  const output = {
    timestamp: new Date().toISOString(),
    wallet: keypair.publicKey.toBase58(),
    gateway: gatewayUrl,
    image: path.basename(absPath),
    image_size_bytes: imageBytes.length,
    verify: {
      duration_ms: verifyMs,
      processors: args.processorIds,
      results: verifyResponse.results.map((r) => ({
        processor_id: r.processor_id,
        signed_json: r.signed_json,
      })),
    },
    sign: {
      duration_ms: signMs,
      blockhash,
      arweave_urls: signRequests.map((r) => r.signed_json_uri),
      partial_txs_count: signResponse.partial_txs.length,
    },
  };

  const outPath = path.resolve("output-register.json");
  fs.writeFileSync(outPath, JSON.stringify(output, null, 2));
  log("DONE", `全結果を保存: ${outPath}`);
}

main().catch((e) => {
  console.error("Fatal:", e);
  process.exit(1);
});
