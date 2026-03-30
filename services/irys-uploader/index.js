// SPDX-License-Identifier: Apache-2.0

/**
 * Irys Uploader Sidecar
 *
 * Title Protocol GatewayからのHTTPリクエストを受け、
 * @irys/upload経由でArweaveにデータを永続保存する。
 *
 * POST /upload
 *   Body: { data: string (base64), content_type: string, private_key: string (base58) }
 *   Response: { url: string }
 *
 * GET /health
 *   Response: { status: "ok" }
 *
 * Environment:
 *   PORT           — Listen port (default: 3001)
 *   SOLANA_RPC_URL — Solana RPC endpoint (default: https://api.devnet.solana.com)
 *   IRYS_NETWORK   — "devnet" or "mainnet" (default: devnet)
 */

import http from "node:http";

const PORT = parseInt(process.env.PORT || "3001", 10);
const SOLANA_RPC_URL =
  process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
const IRYS_NETWORK = process.env.IRYS_NETWORK || "devnet";

/**
 * Irys uploader を初期化する。
 * リクエストごとにインスタンス化（秘密鍵がリクエストごとに渡されるため）。
 */
async function createUploader(privateKeyBase58) {
  const { Uploader } = await import("@irys/upload");
  const { Solana } = await import("@irys/upload-solana");

  let builder = Uploader(Solana)
    .withWallet(privateKeyBase58)
    .withRpc(SOLANA_RPC_URL);

  if (IRYS_NETWORK === "devnet") {
    builder = builder.devnet();
  }

  return builder.build();
}

/**
 * Irysにデータをアップロードし、gateway URLを返す。
 * 残高不足の場合は自動でfundする。
 */
async function uploadToIrys(irys, dataBuffer, contentType) {
  const size = dataBuffer.length;

  // 費用確認
  const price = await irys.getPrice(size);
  const balance = await irys.getBalance();

  if (balance.isLessThan(price)) {
    const deficit = price.minus(balance);
    console.log(
      `[FUND] 残高不足: ${irys.utils.fromAtomic(balance)} < ${irys.utils.fromAtomic(price)} — ${irys.utils.fromAtomic(deficit)} をfund...`
    );
    // 余裕を持って2倍fund
    await irys.fund(price.multipliedBy(2));
    console.log("[FUND] fund完了");
  }

  const tags = [{ name: "Content-Type", value: contentType }];
  const receipt = await irys.upload(dataBuffer, { tags });
  return `https://gateway.irys.xyz/${receipt.id}`;
}

/**
 * リクエストボディをJSONとして読み取る。
 */
function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString()));
      } catch (e) {
        reject(new Error("Invalid JSON"));
      }
    });
    req.on("error", reject);
  });
}

const server = http.createServer(async (req, res) => {
  // Health check
  if (req.method === "GET" && req.url === "/health") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ status: "ok" }));
    return;
  }

  // Upload endpoint
  if (req.method === "POST" && req.url === "/upload") {
    try {
      const body = await readBody(req);

      const { data, content_type, private_key } = body;
      if (!data || !private_key) {
        res.writeHead(400, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "data and private_key are required" }));
        return;
      }

      const dataBuffer = Buffer.from(data, "base64");
      const ct = content_type || "application/octet-stream";

      console.log(
        `[UPLOAD] ${(dataBuffer.length / 1024).toFixed(1)} KB (${ct})`
      );

      const irys = await createUploader(private_key);
      const url = await uploadToIrys(irys, dataBuffer, ct);

      console.log(`[UPLOAD] → ${url}`);

      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ url }));
    } catch (e) {
      console.error("[ERROR]", e.message || e);
      res.writeHead(500, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: e.message || "Internal error" }));
    }
    return;
  }

  res.writeHead(404, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ error: "Not found" }));
});

server.listen(PORT, "0.0.0.0", () => {
  console.log(`[IRYS-UPLOADER] Listening on :${PORT}`);
  console.log(`[IRYS-UPLOADER] RPC: ${SOLANA_RPC_URL}`);
  console.log(`[IRYS-UPLOADER] Network: ${IRYS_NETWORK}`);
});
