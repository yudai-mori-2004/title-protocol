/**
 * E2Eテスト ヘルパー
 *
 * - サービスヘルスチェック
 * - モックウォレット生成
 * - ストレージサーバー（signed_json保管用）
 * - テストフィクスチャ読み込み
 * - TitleClient セットアップ
 */

import * as http from "node:http";
import * as fs from "node:fs";
import * as path from "node:path";
import * as crypto from "node:crypto";

import {
  Connection,
  Keypair,
  Transaction,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";

import {
  TitleClient,
  TitleClientConfig,
  StorageProvider,
  GlobalConfig,
  TrustedTeeNode,
  NodeInfo,
} from "@title-protocol/sdk";

// ---------------------------------------------------------------------------
// 定数
// ---------------------------------------------------------------------------

export const SOLANA_RPC = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
export const GATEWAY_URL = "http://localhost:3000";
export const TEE_URL = "http://localhost:4000";
export const MINIO_URL = "http://localhost:9000";
export const ARLOCAL_URL = "http://localhost:1984";

/**
 * GatewayがDocker内部ホスト名(minio:9000)で返すURLを
 * ホストマシンからアクセス可能なURL(localhost:9000)に変換する。
 */
export function fixMinioUrl(url: string): string {
  return url.replace("http://minio:9000", MINIO_URL);
}

/** ストレージサーバーのポート（signed_json保管用） */
const STORAGE_PORT = 7799;

/** ストレージサーバーの外部URL（TEE Docker内からアクセスする場合） */
export const STORAGE_DOCKER_URL = `http://host.docker.internal:${STORAGE_PORT}`;

/** フィクスチャディレクトリ */
const FIXTURE_DIR = path.join(__dirname, "..", "fixtures");

// ---------------------------------------------------------------------------
// サービスヘルスチェック
// ---------------------------------------------------------------------------

export async function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * 指定URLがHTTP 200を返すまでリトライする。
 */
export async function waitForService(
  url: string,
  name: string,
  maxRetries = 30,
  intervalMs = 2000
): Promise<void> {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {
      // ignore
    }
    if (i < maxRetries - 1) await sleep(intervalMs);
  }
  throw new Error(`${name} の起動がタイムアウトしました (${url})`);
}

/**
 * Solana RPC のヘルスチェック（JSON-RPC getHealth）
 */
async function waitForSolanaRpc(maxRetries = 15): Promise<void> {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const res = await fetch(SOLANA_RPC, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "getHealth",
        }),
      });
      if (res.ok) return;
    } catch {
      // ignore
    }
    if (i < maxRetries - 1) await sleep(2000);
  }
  throw new Error(`Solana RPC の接続がタイムアウトしました (${SOLANA_RPC})`);
}

/**
 * 全サービスの起動を待つ。
 */
export async function waitForAllServices(): Promise<void> {
  await Promise.all([
    waitForService(
      `${GATEWAY_URL}/.well-known/title-node-info`,
      "Gateway",
      30
    ),
    waitForSolanaRpc(15),
  ]);
}

// ---------------------------------------------------------------------------
// モックウォレット
// ---------------------------------------------------------------------------

export interface MockWallet {
  keypair: Keypair;
  publicKey: { toBase58(): string };
  signTransaction(tx: unknown): Promise<unknown>;
}

/**
 * テスト用のSolanaウォレットを作成し、SOLをエアドロップする。
 */
export async function createFundedWallet(): Promise<MockWallet> {
  const keypair = Keypair.generate();
  const connection = new Connection(SOLANA_RPC, "confirmed");

  // Airdrop
  try {
    const sig = await connection.requestAirdrop(
      keypair.publicKey,
      5 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig, "confirmed");
  } catch {
    // test-validator auto-funds or already funded
  }

  return {
    keypair,
    publicKey: {
      toBase58: () => keypair.publicKey.toBase58(),
    },
    signTransaction: async (tx: unknown) => {
      const transaction = tx as Transaction;
      transaction.partialSign(keypair);
      return transaction;
    },
  };
}

// ---------------------------------------------------------------------------
// ストレージサーバー（signed_json保管用）
// ---------------------------------------------------------------------------

/**
 * シンプルなHTTPファイルサーバー。
 * signed_jsonをPOSTで保存し、GETで取得できる。
 * TEEはDocker内から host.docker.internal 経由でアクセスする。
 */
export class TestStorageServer {
  private server: http.Server;
  private store = new Map<string, { data: Buffer; contentType: string }>();
  private port: number;

  constructor(port = STORAGE_PORT) {
    this.port = port;
    this.server = http.createServer((req, res) => {
      if (req.method === "POST" && req.url === "/upload") {
        const chunks: Buffer[] = [];
        req.on("data", (chunk: Buffer) => chunks.push(chunk));
        req.on("end", () => {
          const data = Buffer.concat(chunks);
          const id = crypto.randomUUID();
          const contentType =
            req.headers["content-type"] || "application/octet-stream";
          this.store.set(id, { data, contentType });

          // TEEからアクセス可能なURLを返す
          const uri = `${STORAGE_DOCKER_URL}/data/${id}`;
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ uri }));
        });
      } else if (req.method === "GET" && req.url?.startsWith("/data/")) {
        const id = req.url.slice("/data/".length);
        const entry = this.store.get(id);
        if (entry) {
          res.writeHead(200, { "Content-Type": entry.contentType });
          res.end(entry.data);
        } else {
          res.writeHead(404);
          res.end("Not found");
        }
      } else {
        res.writeHead(404);
        res.end("Not found");
      }
    });
  }

  async start(): Promise<void> {
    return new Promise((resolve) => {
      this.server.listen(this.port, () => resolve());
    });
  }

  async stop(): Promise<void> {
    return new Promise((resolve) => {
      this.server.close(() => resolve());
    });
  }
}

/**
 * TestStorageServerをStorageProvider interfaceでラップする。
 * SDK の register() から使える。
 */
export class TestStorage implements StorageProvider {
  private endpoint: string;

  constructor(port = STORAGE_PORT) {
    this.endpoint = `http://localhost:${port}/upload`;
  }

  async upload(data: Uint8Array, contentType: string): Promise<string> {
    const res = await fetch(this.endpoint, {
      method: "POST",
      headers: { "Content-Type": contentType },
      body: data,
    });
    if (!res.ok) {
      throw new Error(`ストレージアップロード失敗: HTTP ${res.status}`);
    }
    const result = (await res.json()) as { uri: string };
    return result.uri;
  }
}

// ---------------------------------------------------------------------------
// フィクスチャ読み込み
// ---------------------------------------------------------------------------

/**
 * テストフィクスチャを読み込む。
 */
export function loadFixture(name: string): Uint8Array {
  const filePath = path.join(FIXTURE_DIR, name);
  if (!fs.existsSync(filePath)) {
    throw new Error(
      `フィクスチャが見つかりません: ${filePath}\n` +
        `  cargo run --example gen_fixture -p title-core -- ${FIXTURE_DIR} で生成してください`
    );
  }
  return new Uint8Array(fs.readFileSync(filePath));
}

// ---------------------------------------------------------------------------
// TitleClient セットアップ
// ---------------------------------------------------------------------------

/** tee-info.json の型 */
interface TeeInfo {
  tree_address: string;
  signing_pubkey: string;
  encryption_pubkey: string;
}

/**
 * init-config.mjs が保存した tee-info.json を読み込む。
 */
export function loadTeeInfo(): TeeInfo {
  const filePath = path.join(FIXTURE_DIR, "tee-info.json");
  if (!fs.existsSync(filePath)) {
    throw new Error(
      `tee-info.json が見つかりません: ${filePath}\n` +
        `  setup-local.sh を実行してください`
    );
  }
  return JSON.parse(fs.readFileSync(filePath, "utf-8")) as TeeInfo;
}

/**
 * Gateway の node-info + tee-info.json からGlobalConfigモックを構築し
 * TitleClientを返す。
 *
 * 前提: setup-local.sh が完了し、tee-info.json が生成済みであること。
 */
export async function setupClient(
  storage: StorageProvider
): Promise<TitleClient> {
  // tee-info.json から TEE の鍵情報を取得
  const teeInfo = loadTeeInfo();

  // Gateway から node-info を取得（signing_pubkey, supported_extensions, limits）
  const nodeInfoRes = await fetch(
    `${GATEWAY_URL}/.well-known/title-node-info`
  );
  if (!nodeInfoRes.ok) {
    throw new Error(
      `Gateway node-info 取得失敗: HTTP ${nodeInfoRes.status}`
    );
  }
  const nodeInfo = (await nodeInfoRes.json()) as NodeInfo;

  // モックGlobalConfigを構築
  // NOTE: Gateway の signing_pubkey ≠ TEE の signing_pubkey
  // - nodeInfo.signing_pubkey: Gateway の Ed25519 公開鍵
  // - teeInfo.signing_pubkey: TEE の Ed25519 署名用公開鍵
  const teeNode: TrustedTeeNode = {
    signing_pubkey: teeInfo.signing_pubkey,
    encryption_pubkey: teeInfo.encryption_pubkey,
    encryption_algorithm: "x25519-hkdf-sha256-aes256gcm",
    gateway_pubkey: nodeInfo.signing_pubkey,
    gateway_endpoint: GATEWAY_URL,
    status: "active",
    tee_type: "aws_nitro",
    expected_measurements: {},
  };

  const globalConfig: GlobalConfig = {
    authority: "",
    core_collection_mint: "",
    ext_collection_mint: "",
    trusted_tee_nodes: [teeNode],
    trusted_tsa_keys: [],
    trusted_wasm_modules: [
      {
        extension_id: "phash-v1",
        wasm_source: "builtin",
        wasm_hash: "", // MockではWASMハッシュ検証をスキップ
      },
    ],
  };

  const config: TitleClientConfig = {
    teeNodes: [GATEWAY_URL],
    solanaRpcUrl: SOLANA_RPC,
    globalConfig,
    storage,
  };

  return new TitleClient(config);
}
