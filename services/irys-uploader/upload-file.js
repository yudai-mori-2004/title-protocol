#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

/**
 * CLI用 Arweaveアップロードスクリプト。
 *
 * services/irys-uploader/ の依存関係を共有する。
 * title-cli の register-wasm / add-wasm-version から呼び出される。
 *
 * Usage:
 *   node upload-file.js <file-path> <private-key-base58> [content-type]
 *
 * Output (stdout):
 *   JSON: { "url": "https://gateway.irys.xyz/..." }
 *
 * Environment:
 *   SOLANA_RPC_URL — Solana RPC endpoint (default: https://api.devnet.solana.com)
 *   IRYS_NETWORK   — "devnet" or "mainnet" (default: devnet)
 */

import fs from "node:fs";

const SOLANA_RPC_URL =
  process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
const IRYS_NETWORK = process.env.IRYS_NETWORK || "devnet";
const IRYS_GATEWAY = process.env.IRYS_GATEWAY ||
  (IRYS_NETWORK === "devnet" ? "https://devnet.irys.xyz" : "https://gateway.irys.xyz");

async function main() {
  const [, , filePath, privateKey, contentType] = process.argv;

  if (!filePath || !privateKey) {
    console.error(
      "Usage: node upload-file.js <file-path> <private-key-base58> [content-type]"
    );
    process.exit(1);
  }

  const ct = contentType || "application/wasm";
  const dataBuffer = fs.readFileSync(filePath);

  console.error(
    `[irys] Uploading ${(dataBuffer.length / 1024).toFixed(1)} KB (${ct})`
  );
  console.error(`[irys] RPC: ${SOLANA_RPC_URL}`);
  console.error(`[irys] Network: ${IRYS_NETWORK}`);

  // Create uploader
  const { Uploader } = await import("@irys/upload");
  const { Solana } = await import("@irys/upload-solana");

  let builder = Uploader(Solana).withWallet(privateKey).withRpc(SOLANA_RPC_URL);

  if (IRYS_NETWORK === "devnet") {
    builder = builder.devnet();
  }

  const irys = await builder.build();

  // Check balance & fund if needed
  const size = dataBuffer.length;
  const price = await irys.getPrice(size);
  const balance = await irys.getBalance();

  if (balance.isLessThan(price)) {
    const deficit = price.minus(balance);
    console.error(
      `[irys] Balance insufficient: ${irys.utils.fromAtomic(balance)} < ${irys.utils.fromAtomic(price)} — funding ${irys.utils.fromAtomic(deficit)}...`
    );
    await irys.fund(price.multipliedBy(2));
    console.error("[irys] Funded");
  }

  // Upload
  const tags = [{ name: "Content-Type", value: ct }];
  const receipt = await irys.upload(dataBuffer, { tags });
  const url = `${IRYS_GATEWAY}/${receipt.id}`;

  console.error(`[irys] Uploaded → ${url}`);

  // Output JSON to stdout (for CLI to parse)
  console.log(JSON.stringify({ url }));
}

main().catch((e) => {
  console.error(`[irys] ERROR: ${e.message || e}`);
  process.exit(1);
});
