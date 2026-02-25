#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

/**
 * Title Protocol Devnet 完全初期化スクリプト
 *
 * devnet上で「信頼の連鎖」を完成させる:
 * 1. Authority keypair のロードまたは生成
 * 2. MPL Core コレクション作成（Core + Extension）
 * 3. GlobalConfig 初期化（実コレクションMint）
 * 4. TEE ノード情報取得 + 登録
 * 5. WASM モジュール登録
 * 6. Collection Authority 委譲（TEE signing_pubkey へ）
 * 7. Merkle Tree 作成（TEE /create-tree）
 * 8. COLLECTION_MINT を .env に反映
 *
 * 使い方:
 *   cd scripts && npm install
 *   node init-devnet.mjs --gateway http://<EC2_IP>:3000
 *
 * オプション:
 *   --rpc <url>       Solana RPC (default: SOLANA_RPC_URL env or devnet)
 *   --gateway <url>   Gateway URL (default: http://localhost:3000)
 *   --skip-tree       Merkle Tree作成をスキップ
 *   --skip-delegate   Collection Authority委譲をスキップ
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

// Umi + MPL Core
import { createUmi } from "@metaplex-foundation/umi-bundle-defaults";
import {
  keypairIdentity,
  generateSigner,
  createSignerFromKeypair,
  publicKey as umiPublicKey,
} from "@metaplex-foundation/umi";
import { mplCore, createCollection, addCollectionPlugin } from "@metaplex-foundation/mpl-core";

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
const hasFlag = (name) => args.includes(`--${name}`);

const RPC_URL = getArg("rpc", process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com");
const GATEWAY_URL = getArg("gateway", process.env.GATEWAY_URL || "http://localhost:3000");
const SKIP_TREE = hasFlag("skip-tree");
const SKIP_DELEGATE = hasFlag("skip-delegate");

// ---------------------------------------------------------------------------
// 定数
// ---------------------------------------------------------------------------

const PROGRAM_ID = new PublicKey(
  process.env.TITLE_CONFIG_PROGRAM_ID || "C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN"
);

const AUTHORITY_KEY_PATH = join(PROJECT_ROOT, "deploy", "aws", "keys", "devnet-authority.json");

// 4つのWASMモジュール Extension ID
const WASM_MODULES = [
  "phash-v1",
  "hardware-google",
  "c2pa-training-v1",
  "c2pa-license-v1",
];

/** Anchor instruction discriminator = sha256("global:<method>")[..8] */
function anchorDisc(method) {
  return createHash("sha256")
    .update(`global:${method}`)
    .digest()
    .subarray(0, 8);
}

const DISC_INITIALIZE = anchorDisc("initialize");
const DISC_UPDATE_COLLECTIONS = anchorDisc("update_collections");
const DISC_UPDATE_TEE_NODES = anchorDisc("update_tee_nodes");
const DISC_UPDATE_WASM_MODULES = anchorDisc("update_wasm_modules");
const DISC_DELEGATE_COLLECTION_AUTHORITY = anchorDisc("delegate_collection_authority");

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

function findGlobalConfigPDA() {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("global-config")],
    PROGRAM_ID
  );
}

function u32le(n) {
  const buf = Buffer.alloc(4);
  buf.writeUInt32LE(n);
  return buf;
}

async function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function airdropIfNeeded(connection, pubkey, minSol = 2) {
  const balance = await connection.getBalance(pubkey);
  const balanceSol = balance / LAMPORTS_PER_SOL;
  if (balanceSol < minSol) {
    console.log(`    残高 ${balanceSol.toFixed(4)} SOL → Airdrop中...`);
    try {
      const sig = await connection.requestAirdrop(pubkey, 2 * LAMPORTS_PER_SOL);
      await connection.confirmTransaction(sig, "confirmed");
      console.log(`    Airdrop完了 (+2 SOL)`);
    } catch (e) {
      console.log(`    Airdrop失敗（レート制限の可能性）: ${e.message?.substring(0, 80)}`);
      if (balanceSol < 0.01) {
        console.error("    ERROR: SOL残高不足。手動でairdropしてください:");
        console.error(`      solana airdrop 2 ${pubkey.toBase58()} --url ${RPC_URL}`);
        process.exit(1);
      }
    }
  } else {
    console.log(`    残高: ${balanceSol.toFixed(4)} SOL (十分)`);
  }
}

/** Base58デコード */
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
  for (const c of str) {
    if (c !== "1") break;
    bytes.push(0);
  }
  return Buffer.from(bytes.reverse());
}

/** WASM extension_idを32バイトに右パディング */
function extensionIdBytes(id) {
  const buf = Buffer.alloc(32);
  buf.write(id, "utf-8");
  return buf;
}

/** SHA-256ハッシュ (32バイト) */
function sha256Hash(data) {
  return createHash("sha256").update(data).digest();
}

// ---------------------------------------------------------------------------
// Step 1: Authority Keypair
// ---------------------------------------------------------------------------

function loadOrCreateAuthority() {
  if (existsSync(AUTHORITY_KEY_PATH)) {
    console.log(`  既存のキーペアをロード: ${AUTHORITY_KEY_PATH}`);
    const raw = JSON.parse(readFileSync(AUTHORITY_KEY_PATH, "utf-8"));
    return Keypair.fromSecretKey(Uint8Array.from(raw));
  }

  console.log("  新しいキーペアを生成中...");
  const kp = Keypair.generate();
  const dir = dirname(AUTHORITY_KEY_PATH);
  mkdirSync(dir, { recursive: true });
  writeFileSync(AUTHORITY_KEY_PATH, JSON.stringify(Array.from(kp.secretKey)));
  console.log(`  保存先: ${AUTHORITY_KEY_PATH}`);
  return kp;
}

// ---------------------------------------------------------------------------
// Step 3: MPL Core Collection
// ---------------------------------------------------------------------------

async function createMplCoreCollection(umi, name, uri) {
  const collectionSigner = generateSigner(umi);
  console.log(`    Collection address: ${collectionSigner.publicKey}`);

  const builder = createCollection(umi, {
    collection: collectionSigner,
    name,
    uri,
  });

  const result = await builder.sendAndConfirm(umi);
  console.log(`    作成完了 (sig: ${Buffer.from(result.signature).toString("base64").substring(0, 20)}...)`);
  return collectionSigner.publicKey; // base58 string
}

// ---------------------------------------------------------------------------
// メイン
// ---------------------------------------------------------------------------

async function main() {
  console.log("=== Title Protocol Devnet 完全初期化 ===\n");
  console.log(`  RPC: ${RPC_URL}`);
  console.log(`  Gateway: ${GATEWAY_URL}`);
  console.log(`  Program: ${PROGRAM_ID.toBase58()}\n`);

  const connection = new Connection(RPC_URL, "confirmed");

  // =====================================================================
  // Step 1: Authority Keypair
  // =====================================================================
  console.log("[Step 1] Authority Keypair");
  const authority = loadOrCreateAuthority();
  console.log(`  Authority: ${authority.publicKey.toBase58()}`);

  // =====================================================================
  // Step 2: Airdrop
  // =====================================================================
  console.log("\n[Step 2] Airdrop");
  await airdropIfNeeded(connection, authority.publicKey, 2);

  // =====================================================================
  // Step 3: MPL Core Collections
  // =====================================================================
  console.log("\n[Step 3] MPL Core コレクション作成");

  // Umi インスタンス生成
  const umi = createUmi(RPC_URL);
  umi.use(mplCore());

  // Authority keypairをUmiに登録
  const umiKeypair = {
    publicKey: umiPublicKey(authority.publicKey.toBase58()),
    secretKey: authority.secretKey,
  };
  umi.use(keypairIdentity(umiKeypair));

  // 既存のGlobalConfig確認 → コレクションが既に存在するかチェック
  const [globalConfigPda] = findGlobalConfigPDA();
  console.log(`  Global Config PDA: ${globalConfigPda.toBase58()}`);

  let coreMintStr, extMintStr;
  const existingAccount = await connection.getAccountInfo(globalConfigPda);

  if (existingAccount) {
    // 既存のGlobalConfigからコレクションMintを読み取り
    // Anchor account: 8-byte discriminator + 32 authority + 32 core_collection_mint + 32 ext_collection_mint ...
    const data = existingAccount.data;
    const coreMintBytes = data.subarray(8 + 32, 8 + 32 + 32);
    const extMintBytes = data.subarray(8 + 32 + 32, 8 + 32 + 32 + 32);
    const coreMintPk = new PublicKey(coreMintBytes);
    const extMintPk = new PublicKey(extMintBytes);

    // ダミー値かどうか判定: 有効なMPL Coreアカウントが存在するか
    const coreAcct = await connection.getAccountInfo(coreMintPk);
    const extAcct = await connection.getAccountInfo(extMintPk);

    if (coreAcct && extAcct) {
      console.log("  既存のコレクションを使用:");
      console.log(`    Core:      ${coreMintPk.toBase58()}`);
      console.log(`    Extension: ${extMintPk.toBase58()}`);
      coreMintStr = coreMintPk.toBase58();
      extMintStr = extMintPk.toBase58();
    } else {
      // コレクションが無効（ダミー値） → 作成して update_collections で更新
      console.log("  コレクションが無効。新規作成 → update_collections で更新します。");

      console.log("  Core Collection 作成中...");
      coreMintStr = await createMplCoreCollection(umi, "Title Protocol Core", "");

      console.log("  Extension Collection 作成中...");
      extMintStr = await createMplCoreCollection(umi, "Title Protocol Extension", "");

      // update_collections でGlobalConfigを更新
      console.log("  GlobalConfig のコレクションMintを更新中...");
      const newCorePk = new PublicKey(coreMintStr);
      const newExtPk = new PublicKey(extMintStr);

      const updateData = Buffer.concat([
        DISC_UPDATE_COLLECTIONS,
        newCorePk.toBuffer(),
        newExtPk.toBuffer(),
      ]);

      const updateIx = new TransactionInstruction({
        keys: [
          { pubkey: globalConfigPda, isSigner: false, isWritable: true },
          { pubkey: authority.publicKey, isSigner: true, isWritable: false },
        ],
        programId: PROGRAM_ID,
        data: updateData,
      });

      const updateTx = new Transaction().add(updateIx);
      const updateSig = await sendAndConfirmTransaction(connection, updateTx, [authority]);
      console.log(`  update_collections 完了: ${updateSig}`);
      console.log(`    core_collection_mint: ${coreMintStr}`);
      console.log(`    ext_collection_mint:  ${extMintStr}`);
    }
  } else {
    // GlobalConfig未初期化 → コレクション作成 → initialize
    console.log("  Core Collection 作成中...");
    coreMintStr = await createMplCoreCollection(umi, "Title Protocol Core", "");

    console.log("  Extension Collection 作成中...");
    extMintStr = await createMplCoreCollection(umi, "Title Protocol Extension", "");

    console.log("\n[Step 4] GlobalConfig 初期化");
    const coreMintPk = new PublicKey(coreMintStr);
    const extMintPk = new PublicKey(extMintStr);

    const initData = Buffer.concat([
      DISC_INITIALIZE,
      coreMintPk.toBuffer(),
      extMintPk.toBuffer(),
    ]);

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: globalConfigPda, isSigner: false, isWritable: true },
        { pubkey: authority.publicKey, isSigner: true, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId: PROGRAM_ID,
      data: initData,
    });

    const tx = new Transaction().add(ix);
    const sig = await sendAndConfirmTransaction(connection, tx, [authority]);
    console.log(`  GlobalConfig 初期化完了: ${sig}`);
    console.log(`    core_collection_mint: ${coreMintStr}`);
    console.log(`    ext_collection_mint:  ${extMintStr}`);
  }

  // =====================================================================
  // Step 5: TEE ノード情報取得
  // =====================================================================
  console.log("\n[Step 5] TEE ノード情報取得");

  let teeSigningPubkey, teeEncryptionPubkey, gatewayPubkey;

  // Gateway /.well-known/title-node-info からGateway公開鍵を取得
  try {
    const res = await fetch(`${GATEWAY_URL}/.well-known/title-node-info`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const nodeInfo = await res.json();
    gatewayPubkey = nodeInfo.signing_pubkey; // Base58
    console.log(`  Gateway signing_pubkey: ${gatewayPubkey}`);
  } catch (e) {
    console.log(`  WARNING: Gateway に接続できません: ${e.message}`);
    console.log("  → TEEノード登録をスキップします。Gatewayが稼働後に再実行してください。");
    gatewayPubkey = null;
  }

  // TEE signing_pubkey と encryption_pubkey は /create-tree レスポンスから取得
  // まずは tee-info.json があれば読む
  const teeInfoPath = join(PROJECT_ROOT, "tests", "e2e", "fixtures", "tee-info.json");
  if (existsSync(teeInfoPath)) {
    const teeInfo = JSON.parse(readFileSync(teeInfoPath, "utf-8"));
    teeSigningPubkey = teeInfo.signing_pubkey;
    teeEncryptionPubkey = teeInfo.encryption_pubkey;
    console.log(`  TEE signing_pubkey (cached): ${teeSigningPubkey}`);
    console.log(`  TEE encryption_pubkey (cached): ${teeEncryptionPubkey}`);
  }

  // =====================================================================
  // Step 6: TEE ノード登録 (update_tee_nodes)
  // =====================================================================
  if (teeSigningPubkey && gatewayPubkey) {
    console.log("\n[Step 6] TEE ノード登録");

    const signingBytes = bs58Decode(teeSigningPubkey);
    const encryptionBytes = teeEncryptionPubkey
      ? Buffer.from(teeEncryptionPubkey, "base64")
      : Buffer.alloc(32);
    const gatewayBytes = bs58Decode(gatewayPubkey);

    // TrustedTeeNodeAccount: signing_pubkey(32) + encryption_pubkey(32) + gateway_pubkey(32) + status(1) + tee_type(1)
    const nodeData = Buffer.concat([
      signingBytes,
      encryptionBytes,
      gatewayBytes,
      Buffer.from([1]),  // status: Active
      Buffer.from([0]),  // tee_type: aws_nitro
    ]);

    const data = Buffer.concat([
      DISC_UPDATE_TEE_NODES,
      u32le(1),  // 1 node
      nodeData,
    ]);

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: globalConfigPda, isSigner: false, isWritable: true },
        { pubkey: authority.publicKey, isSigner: true, isWritable: false },
      ],
      programId: PROGRAM_ID,
      data,
    });

    const tx = new Transaction().add(ix);
    try {
      const sig = await sendAndConfirmTransaction(connection, tx, [authority]);
      console.log(`  TEEノード登録完了: ${sig}`);
    } catch (e) {
      console.log(`  TEEノード登録失敗: ${e.message?.substring(0, 100)}`);
    }
  } else {
    console.log("\n[Step 6] TEE ノード登録 → スキップ（TEE情報が不足）");
    if (!teeSigningPubkey) {
      console.log("  → TEE signing_pubkey が未取得。/create-tree 後に再実行してください。");
    }
  }

  // =====================================================================
  // Step 7: WASM モジュール登録 (update_wasm_modules)
  // =====================================================================
  console.log("\n[Step 7] WASM モジュール登録");

  // WASMバイナリのハッシュを計算
  // Docker内 /wasm-modules/{id}.wasm に配置されるが、ローカルにはビルド済みバイナリがない場合もある
  const wasmModules = [];
  let allWasmFound = true;

  for (const moduleId of WASM_MODULES) {
    // ローカルビルド済みパス
    const localPath = join(
      PROJECT_ROOT, "wasm", moduleId,
      "target", "wasm32-unknown-unknown", "release",
      `${moduleId.replace(/-/g, "_")}.wasm`
    );

    if (existsSync(localPath)) {
      const wasmBytes = readFileSync(localPath);
      const hash = sha256Hash(wasmBytes);
      wasmModules.push({ id: moduleId, hash });
      console.log(`  ${moduleId}: ${hash.toString("hex").substring(0, 16)}... (${wasmBytes.length} bytes)`);
    } else {
      console.log(`  ${moduleId}: ローカルビルドなし (${localPath})`);
      allWasmFound = false;
    }
  }

  if (wasmModules.length > 0) {
    // TrustedWasmModuleAccount: extension_id(32) + wasm_hash(32)
    const moduleData = wasmModules.map((m) =>
      Buffer.concat([extensionIdBytes(m.id), m.hash])
    );

    const data = Buffer.concat([
      DISC_UPDATE_WASM_MODULES,
      u32le(wasmModules.length),
      ...moduleData,
    ]);

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: globalConfigPda, isSigner: false, isWritable: true },
        { pubkey: authority.publicKey, isSigner: true, isWritable: false },
      ],
      programId: PROGRAM_ID,
      data,
    });

    const tx = new Transaction().add(ix);
    try {
      const sig = await sendAndConfirmTransaction(connection, tx, [authority]);
      console.log(`  WASMモジュール登録完了: ${sig}`);
    } catch (e) {
      console.log(`  WASMモジュール登録失敗: ${e.message?.substring(0, 100)}`);
    }
  }

  if (!allWasmFound) {
    console.log("  NOTE: 一部WASMバイナリが見つかりません。EC2上でビルド後に再実行してください:");
    console.log("    cd wasm/<module> && cargo build --target wasm32-unknown-unknown --release");
  }

  // =====================================================================
  // Step 8: Collection Authority 委譲
  // =====================================================================
  if (!SKIP_DELEGATE && teeSigningPubkey && coreMintStr && extMintStr) {
    console.log("\n[Step 8] Collection Authority 委譲");

    const teePubkey = new PublicKey(teeSigningPubkey);

    // Anchor delegate_collection_authority は「イベント発行のみ」（CPI無し）
    // 実際のMPL Core UpdateDelegateプラグイン追加はクライアントサイドで行う
    for (const [collectionType, mintStr, label] of [
      [0, coreMintStr, "Core"],
      [1, extMintStr, "Extension"],
    ]) {
      console.log(`  ${label} Collection: ${mintStr}`);

      // 1. Anchor命令: delegate_collection_authority（オンチェーン記録）
      const anchorData = Buffer.concat([
        DISC_DELEGATE_COLLECTION_AUTHORITY,
        teePubkey.toBuffer(),            // tee_signing_pubkey: Pubkey
        Buffer.from([collectionType]),   // collection_type: u8
      ]);

      const anchorIx = new TransactionInstruction({
        keys: [
          { pubkey: globalConfigPda, isSigner: false, isWritable: false },
          { pubkey: authority.publicKey, isSigner: true, isWritable: false },
          { pubkey: new PublicKey(mintStr), isSigner: false, isWritable: false },
        ],
        programId: PROGRAM_ID,
        data: anchorData,
      });

      // 2. MPL Core: addCollectionPlugin で UpdateDelegate を追加
      //    これにより TEE signing_pubkey がコレクションの Update Authority を委任される
      try {
        const pluginBuilder = addCollectionPlugin(umi, {
          collection: umiPublicKey(mintStr),
          plugin: {
            type: "UpdateDelegate",
            additionalDelegates: [],
            authority: {
              type: "UpdateAuthority",
            },
          },
        });

        // Anchor命令とMPL Core命令を同一トランザクションで送信
        // まずAnchor命令をweb3.jsで送信（イベント記録用）
        const anchorTx = new Transaction().add(anchorIx);
        try {
          const anchorSig = await sendAndConfirmTransaction(connection, anchorTx, [authority]);
          console.log(`    Anchor delegate記録: ${anchorSig}`);
        } catch (e) {
          console.log(`    Anchor delegate記録失敗（既に登録済みの可能性）: ${e.message?.substring(0, 80)}`);
        }

        // MPL Core UpdateDelegate プラグイン追加
        try {
          const pluginResult = await pluginBuilder.sendAndConfirm(umi);
          console.log(`    MPL Core UpdateDelegate追加完了`);
        } catch (e) {
          console.log(`    MPL Core UpdateDelegate追加失敗（既に存在の可能性）: ${e.message?.substring(0, 80)}`);
        }
      } catch (e) {
        console.log(`    Collection Authority委譲失敗: ${e.message?.substring(0, 100)}`);
      }
    }
  } else if (SKIP_DELEGATE) {
    console.log("\n[Step 8] Collection Authority 委譲 → スキップ (--skip-delegate)");
  } else {
    console.log("\n[Step 8] Collection Authority 委譲 → スキップ（TEE情報またはコレクション情報が不足）");
  }

  // =====================================================================
  // Step 9: Merkle Tree 作成 (TEE /create-tree)
  // =====================================================================
  if (!SKIP_TREE) {
    console.log("\n[Step 9] Merkle Tree 作成");

    // TEE URL を Gateway URL から推測（同一ホスト、ポート4000）
    // ただしEC2ではTEEポートは外部からアクセス不可。Gateway経由を試す。
    // TEE /create-tree は Gateway からプロキシされない独立エンドポイント。
    // EC2上のsetup-ec2.shでTEEはlocalhost:4000で稼働している。
    // リモートの場合はGateway経由でアクセスする方法がない → EC2上で直接実行する必要がある。

    // まず Gateway /create-tree を試す（Gateway がプロキシしている場合）
    let createTreeUrl = `${GATEWAY_URL}/create-tree`;
    let teeUrl = GATEWAY_URL.replace(/:3000$/, ":4000");

    const { blockhash } = await connection.getLatestBlockhash();

    // TEE signing_pubkey が未知の場合、先にTEE walletに送金できないため
    // /create-tree は TEE wallet に SOL がある前提で呼ぶ
    let treeResult = null;

    for (const url of [createTreeUrl, `${teeUrl}/create-tree`]) {
      try {
        console.log(`  /create-tree を呼び出し中: ${url}`);
        const res = await fetch(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            max_depth: 14,
            max_buffer_size: 64,
            recent_blockhash: blockhash,
          }),
          signal: AbortSignal.timeout(15000),
        });

        if (res.ok) {
          treeResult = await res.json();
          console.log(`  Tree Address: ${treeResult.tree_address}`);
          console.log(`  TEE Signing Pubkey: ${treeResult.signing_pubkey}`);
          console.log(`  TEE Encryption Pubkey: ${treeResult.encryption_pubkey}`);

          // tee-info.json を保存
          const teeInfoDir = join(PROJECT_ROOT, "tests", "e2e", "fixtures");
          mkdirSync(teeInfoDir, { recursive: true });
          writeFileSync(
            teeInfoPath,
            JSON.stringify({
              tree_address: treeResult.tree_address,
              signing_pubkey: treeResult.signing_pubkey,
              encryption_pubkey: treeResult.encryption_pubkey,
            }, null, 2)
          );
          console.log(`  TEE情報を保存: ${teeInfoPath}`);

          // TEE wallet に SOL 送金
          const teePk = new PublicKey(treeResult.signing_pubkey);
          const teeBalance = await connection.getBalance(teePk);
          const REQUIRED = 0.5 * LAMPORTS_PER_SOL;
          if (teeBalance < REQUIRED) {
            const amount = REQUIRED - teeBalance;
            console.log(`  TEE wallet にSOL送金中... (${amount / LAMPORTS_PER_SOL} SOL)`);
            const transferIx = SystemProgram.transfer({
              fromPubkey: authority.publicKey,
              toPubkey: teePk,
              lamports: amount,
            });
            const transferTx = new Transaction().add(transferIx);
            try {
              const transferSig = await sendAndConfirmTransaction(connection, transferTx, [authority]);
              console.log(`  SOL送金完了: ${transferSig}`);
            } catch (e) {
              console.log(`  SOL送金失敗: ${e.message?.substring(0, 100)}`);
            }
          } else {
            console.log(`  TEE wallet 残高: ${teeBalance / LAMPORTS_PER_SOL} SOL (十分)`);
          }

          // signed_tx をブロードキャスト
          const txBytes = Buffer.from(treeResult.signed_tx, "base64");
          const signedTx = Transaction.from(txBytes);
          try {
            const txSig = await connection.sendRawTransaction(signedTx.serialize());
            await connection.confirmTransaction(txSig, "confirmed");
            console.log(`  Merkle Tree 作成完了: ${txSig}`);
          } catch (e) {
            console.log(`  Merkle Tree ブロードキャスト失敗: ${e.message?.substring(0, 120)}`);
            console.log("  TEE walletの残高不足の可能性があります。以下を実行後にリトライ:");
            console.log(`    solana transfer ${treeResult.signing_pubkey} 0.5 --allow-unfunded-recipient --url ${RPC_URL}`);
          }

          break; // 成功したらループを抜ける
        } else {
          const body = await res.text();
          console.log(`  ${url}: HTTP ${res.status} - ${body.substring(0, 100)}`);
        }
      } catch (e) {
        console.log(`  ${url}: 接続失敗 (${e.message?.substring(0, 60)})`);
      }
    }

    if (!treeResult) {
      console.log("  WARNING: Merkle Tree の作成に失敗しました。");
      console.log("  EC2上で直接実行する場合:");
      console.log(`    curl -X POST http://localhost:4000/create-tree \\`);
      console.log(`      -H 'Content-Type: application/json' \\`);
      console.log(`      -d '{"max_depth":14,"max_buffer_size":64,"recent_blockhash":"<blockhash>"}'`);
    }

    // TEE情報が新たに取得できた場合、Step 6 の TEEノード登録を改めて実行
    if (treeResult && !teeSigningPubkey) {
      teeSigningPubkey = treeResult.signing_pubkey;
      teeEncryptionPubkey = treeResult.encryption_pubkey;

      if (gatewayPubkey) {
        console.log("\n  [追加] TEEノード情報を登録...");
        const signingBytes = bs58Decode(teeSigningPubkey);
        const encryptionBytes = Buffer.from(teeEncryptionPubkey, "base64");
        const gatewayBytes = bs58Decode(gatewayPubkey);

        const nodeData = Buffer.concat([
          signingBytes, encryptionBytes, gatewayBytes,
          Buffer.from([1]), Buffer.from([0]),
        ]);
        const data = Buffer.concat([DISC_UPDATE_TEE_NODES, u32le(1), nodeData]);

        const ix = new TransactionInstruction({
          keys: [
            { pubkey: globalConfigPda, isSigner: false, isWritable: true },
            { pubkey: authority.publicKey, isSigner: true, isWritable: false },
          ],
          programId: PROGRAM_ID,
          data,
        });

        const tx = new Transaction().add(ix);
        try {
          const sig = await sendAndConfirmTransaction(connection, tx, [authority]);
          console.log(`  TEEノード登録完了: ${sig}`);
        } catch (e) {
          console.log(`  TEEノード登録失敗: ${e.message?.substring(0, 100)}`);
        }
      }
    }
  } else {
    console.log("\n[Step 9] Merkle Tree 作成 → スキップ (--skip-tree)");
  }

  // =====================================================================
  // Step 10: .env 更新ガイダンス
  // =====================================================================
  console.log("\n[Step 10] .env 更新ガイダンス");
  console.log("  以下の値を .env に設定してください:");
  console.log(`    COLLECTION_MINT=${coreMintStr}`);
  if (teeEncryptionPubkey) {
    console.log(`    TEE_ENCRYPTION_PUBKEY=${teeEncryptionPubkey}`);
  }
  if (gatewayPubkey) {
    console.log(`    GATEWAY_PUBKEY=${gatewayPubkey}`);
  }

  // EC2上の .env に COLLECTION_MINT を書き込みガイダンス
  if (coreMintStr) {
    console.log("\n  EC2上のTEEを更新する場合:");
    console.log(`    ssh ec2-user@<IP> "echo 'COLLECTION_MINT=${coreMintStr}' >> ~/title-protocol/.env"`);
    console.log("    → Enclave再起動が必要です");
  }

  // =====================================================================
  // サマリー
  // =====================================================================
  console.log("\n=== 初期化サマリー ===");
  console.log(`  Authority:            ${authority.publicKey.toBase58()}`);
  console.log(`  Authority keypair:    ${AUTHORITY_KEY_PATH}`);
  console.log(`  GlobalConfig PDA:     ${globalConfigPda.toBase58()}`);
  console.log(`  Core Collection:      ${coreMintStr || "(未作成)"}`);
  console.log(`  Extension Collection: ${extMintStr || "(未作成)"}`);
  if (teeSigningPubkey) {
    console.log(`  TEE Signing Pubkey:   ${teeSigningPubkey}`);
  }
  if (teeEncryptionPubkey) {
    console.log(`  TEE Encryption Pubkey: ${teeEncryptionPubkey}`);
  }
  console.log(`  Program ID:           ${PROGRAM_ID.toBase58()}`);
  console.log("");
}

main().catch((e) => {
  console.error("\nFATAL:", e);
  process.exit(1);
});
