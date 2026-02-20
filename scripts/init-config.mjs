#!/usr/bin/env node
/**
 * Title Protocol ローカル開発環境 Global Config初期化スクリプト
 *
 * setup-local.sh から呼ばれ、以下を実行する:
 * 1. Global Config PDA を Anchor プログラム経由で初期化
 * 2. TEE mock の /.well-known/title-node-info からノード情報取得
 * 3. Global Config に TEE ノード情報を登録
 * 4. TEE /create-tree を呼び出し Merkle Tree を作成
 *
 * 使い方:
 *   node scripts/init-config.mjs --rpc http://localhost:8899 --gateway http://localhost:3000 --tee http://localhost:4000
 */

import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  LAMPORTS_PER_SOL,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { createHash } from "crypto";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "fs";
import { homedir } from "os";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PROJECT_ROOT = dirname(__dirname);

// ---------------------------------------------------------------------------
// 引数パース
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
function getArg(name, defaultVal) {
  const idx = args.indexOf(`--${name}`);
  if (idx >= 0 && idx + 1 < args.length) return args[idx + 1];
  return defaultVal;
}

const RPC_URL = getArg("rpc", process.env.SOLANA_RPC_URL || "http://localhost:8899");
const GATEWAY_URL = getArg("gateway", "http://localhost:3000");
const TEE_URL = getArg("tee", "http://localhost:4000");

// ---------------------------------------------------------------------------
// 定数
// ---------------------------------------------------------------------------

const PROGRAM_ID = new PublicKey(
  "C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN"
);

/** Anchor instruction discriminator = sha256("global:<method>")[..8] */
function anchorDisc(method) {
  return createHash("sha256")
    .update(`global:${method}`)
    .digest()
    .subarray(0, 8);
}

const DISC_INITIALIZE = anchorDisc("initialize");
const DISC_UPDATE_TEE_NODES = anchorDisc("update_tee_nodes");

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

function loadKeypair() {
  const keyPath = join(homedir(), ".config", "solana", "id.json");
  if (!existsSync(keyPath)) {
    console.error(`ERROR: Solana キーペアが見つかりません: ${keyPath}`);
    process.exit(1);
  }
  const raw = JSON.parse(readFileSync(keyPath, "utf-8"));
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

function findGlobalConfigPDA() {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("global-config")],
    PROGRAM_ID
  );
}

/** u32 LE encode */
function u32le(n) {
  const buf = Buffer.alloc(4);
  buf.writeUInt32LE(n);
  return buf;
}

async function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function waitForService(url, name, maxRetries = 30) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {
      // ignore
    }
    await sleep(2000);
  }
  console.error(`ERROR: ${name} の起動がタイムアウトしました (${url})`);
}

// ---------------------------------------------------------------------------
// メイン
// ---------------------------------------------------------------------------

async function main() {
  const connection = new Connection(RPC_URL, "confirmed");
  const wallet = loadKeypair();
  console.log(`  Authority: ${wallet.publicKey.toBase58()}`);

  // Airdrop
  try {
    const sig = await connection.requestAirdrop(
      wallet.publicKey,
      10 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig, "confirmed");
  } catch {
    // Already funded or test-validator auto-funds
  }

  const [globalConfigPda] = findGlobalConfigPDA();
  console.log(`  Global Config PDA: ${globalConfigPda.toBase58()}`);

  // -----------------------------------------------------------------------
  // 1. Initialize Global Config (if not already initialized)
  // -----------------------------------------------------------------------
  const accountInfo = await connection.getAccountInfo(globalConfigPda);

  if (!accountInfo) {
    console.log("  Global Config を初期化中...");

    // ダミーのコレクションMint（ローカル開発用）
    const coreMint = Keypair.generate().publicKey;
    const extMint = Keypair.generate().publicKey;

    const data = Buffer.concat([
      DISC_INITIALIZE,
      coreMint.toBuffer(),
      extMint.toBuffer(),
    ]);

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: globalConfigPda, isSigner: false, isWritable: true },
        { pubkey: wallet.publicKey, isSigner: true, isWritable: true },
        {
          pubkey: SystemProgram.programId,
          isSigner: false,
          isWritable: false,
        },
      ],
      programId: PROGRAM_ID,
      data,
    });

    const tx = new Transaction().add(ix);
    try {
      await sendAndConfirmTransaction(connection, tx, [wallet]);
      console.log("  Global Config 初期化完了");
    } catch (e) {
      console.log(`  Global Config 初期化スキップ（既に存在するか、プログラム未デプロイ）: ${e.message?.substring(0, 80)}`);
    }
  } else {
    console.log("  Global Config は既に存在します");
  }

  // -----------------------------------------------------------------------
  // 2. Gateway のノード情報を取得
  // -----------------------------------------------------------------------
  console.log("  Gateway ノード情報を取得中...");
  await waitForService(
    `${GATEWAY_URL}/.well-known/title-node-info`,
    "Gateway"
  );

  let nodeInfo;
  try {
    const res = await fetch(`${GATEWAY_URL}/.well-known/title-node-info`);
    nodeInfo = await res.json();
    console.log(`  TEE signing_pubkey: ${nodeInfo.signing_pubkey}`);
  } catch (e) {
    console.log(
      `  WARNING: Gateway からノード情報を取得できません: ${e.message}`
    );
    console.log("  TEEノード登録をスキップします");
    return;
  }

  // -----------------------------------------------------------------------
  // 3. Update TEE Nodes in Global Config
  // -----------------------------------------------------------------------
  if (accountInfo || (await connection.getAccountInfo(globalConfigPda))) {
    console.log("  TEEノード情報を登録中...");

    // TrustedTeeNodeAccount: signing_pubkey(32) + encryption_pubkey(32) + gateway_pubkey(32) + status(1) + tee_type(1) = 98 bytes
    // signing_pubkeyはBase58, encryption_pubkeyはBase64, gateway_pubkeyはBase58

    // NOTE: Gateway の node-info は signing_pubkey のみ返すため、
    // encryption_pubkey は TEE の /create-tree レスポンスから取得する必要がある。
    // ここではダミー値を設定し、/create-tree 後に更新する。
    const signingPubkeyBytes = bs58Decode(nodeInfo.signing_pubkey);
    const dummyEncPubkey = Buffer.alloc(32); // TEE の /create-tree 後に更新
    const dummyGatewayPubkey = Buffer.alloc(32);

    const nodeData = Buffer.concat([
      signingPubkeyBytes,
      dummyEncPubkey,
      dummyGatewayPubkey,
      Buffer.from([1]), // status: Active
      Buffer.from([0]), // tee_type: aws_nitro (mock)
    ]);

    const data = Buffer.concat([
      DISC_UPDATE_TEE_NODES,
      u32le(1), // 1 node
      nodeData,
    ]);

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: globalConfigPda, isSigner: false, isWritable: true },
        { pubkey: wallet.publicKey, isSigner: true, isWritable: false },
      ],
      programId: PROGRAM_ID,
      data,
    });

    const tx = new Transaction().add(ix);
    try {
      await sendAndConfirmTransaction(connection, tx, [wallet]);
      console.log("  TEEノード登録完了");
    } catch (e) {
      console.log(`  TEEノード登録スキップ: ${e.message?.substring(0, 80)}`);
    }
  }

  // -----------------------------------------------------------------------
  // 4. TEE /create-tree
  // -----------------------------------------------------------------------
  console.log("  Merkle Tree 作成中...");
  await waitForService(`${TEE_URL}/health`, "TEE Mock", 15);

  const { blockhash } = await connection.getLatestBlockhash();

  try {
    const createTreeRes = await fetch(`${TEE_URL}/create-tree`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        max_depth: 14,
        max_buffer_size: 64,
        recent_blockhash: blockhash,
        payer: wallet.publicKey.toBase58(),
      }),
    });

    if (createTreeRes.ok) {
      const result = await createTreeRes.json();
      console.log(`  Tree Address: ${result.tree_address}`);
      console.log(`  Signing Pubkey: ${result.signing_pubkey}`);
      console.log(`  Encryption Pubkey: ${result.encryption_pubkey}`);

      // TEE情報をE2Eテスト用に保存
      const teeInfoDir = join(PROJECT_ROOT, "tests", "e2e", "fixtures");
      try {
        mkdirSync(teeInfoDir, { recursive: true });
        writeFileSync(
          join(teeInfoDir, "tee-info.json"),
          JSON.stringify({
            tree_address: result.tree_address,
            signing_pubkey: result.signing_pubkey,
            encryption_pubkey: result.encryption_pubkey,
          }, null, 2)
        );
        console.log(`  TEE情報を保存: ${join(teeInfoDir, "tee-info.json")}`);
      } catch {
        // tests/e2e ディレクトリが存在しない場合はスキップ
      }

      // partial_tx に署名してブロードキャスト
      const txBytes = Buffer.from(result.partial_tx, "base64");
      const { Transaction: SolTx } = await import("@solana/web3.js");
      const partialTx = SolTx.from(txBytes);
      partialTx.partialSign(wallet);

      try {
        const sig = await connection.sendRawTransaction(
          partialTx.serialize()
        );
        await connection.confirmTransaction(sig, "confirmed");
        console.log(`  Merkle Tree 作成完了: ${sig}`);
      } catch (e) {
        console.log(`  Merkle Tree ブロードキャスト失敗（再試行が必要な場合あり）: ${e.message?.substring(0, 80)}`);
      }
    } else {
      const body = await createTreeRes.text();
      console.log(`  /create-tree 失敗: HTTP ${createTreeRes.status} - ${body.substring(0, 100)}`);
    }
  } catch (e) {
    console.log(`  /create-tree 呼び出し失敗: ${e.message}`);
  }
}

// ---------------------------------------------------------------------------
// Base58
// ---------------------------------------------------------------------------

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function bs58Decode(str) {
  const bytes = [];
  for (const c of str) {
    let carry = ALPHABET.indexOf(c);
    if (carry < 0) throw new Error(`Invalid base58 character: ${c}`);
    for (let j = 0; j < bytes.length; j++) {
      carry += bytes[j] * 58;
      bytes[j] = carry & 0xff;
      carry >>= 8;
    }
    while (carry > 0) {
      bytes.push(carry & 0xff);
      carry >>= 8;
    }
  }
  // Leading zeros
  for (const c of str) {
    if (c !== "1") break;
    bytes.push(0);
  }
  return Buffer.from(bytes.reverse());
}

main().catch((e) => {
  console.error("ERROR:", e);
  process.exit(1);
});
