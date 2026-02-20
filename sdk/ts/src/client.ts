/**
 * TitleClient — SDK のメインクラス
 *
 * 仕様書 §6.7
 *
 * TEEノードはフラットなURL配列で管理される。
 * 各URLは `https://<gateway-host>?apikey=<key>` のようなフラット形式。
 * SDKは配列からランダムにノードを選択するが、
 * 暗号化アップロード後はそのノードにセッションが紐付く（アフィニティ）。
 */

import type {
  GlobalConfig,
  TrustedTeeNode,
  TrustedWasmModule,
  VerifyRequest,
  VerifyResponse,
  SignRequest,
  SignResponse,
  EncryptedPayload,
  RegisterResult,
  ResolveResult,
  NodeInfo,
} from "./types";
import { StorageProvider } from "./storage";

// ---------------------------------------------------------------------------
// 設定
// ---------------------------------------------------------------------------

/** TitleClient 初期化オプション */
export interface TitleClientConfig {
  /**
   * TEEノードのURL一覧（フラット配列）。
   * 各URLは `https://gateway.example.com?apikey=xxx` のような形式。
   * SDKはこの中からランダムにノードを選択する。
   */
  teeNodes: string[];

  /** Solana RPC URL */
  solanaRpcUrl: string;

  /**
   * モックGlobalConfig。
   * 本番ではSolana RPCから取得するが、現時点ではGlobalConfigのPDAが
   * まだデプロイされていないため、外部から注入する。
   */
  globalConfig: GlobalConfig;

  /** オフチェーンストレージプロバイダ（Arweave等） */
  storage: StorageProvider;
}

// ---------------------------------------------------------------------------
// ノードセッション（アフィニティ管理）
// ---------------------------------------------------------------------------

/**
 * 特定のTEEノードとのセッション。
 * 暗号化アップロード後、同一ノードに対してverify/signを行う。
 */
export interface TeeSession {
  /** このセッションで使用するGateway URL */
  gatewayUrl: string;
  /** TEEのX25519暗号化公開鍵（Base64） */
  encryptionPubkey: string;
  /** TEEのEd25519署名公開鍵（Base58） */
  signingPubkey: string;
}

// ---------------------------------------------------------------------------
// TitleClient
// ---------------------------------------------------------------------------

export class TitleClient {
  readonly config: TitleClientConfig;

  constructor(config: TitleClientConfig) {
    if (config.teeNodes.length === 0) {
      throw new Error("teeNodesは1つ以上のURLを含む必要があります");
    }
    this.config = config;
  }

  /**
   * TEEノードをランダムに1つ選択し、セッションを開始する。
   * 暗号化アップロードを行う前にこれを呼ぶ。
   * 返却されたTeeSessionを後続のregister()に渡すことで、
   * 同一TEEノードへのアフィニティが保証される。
   *
   * 仕様書 §6.7
   */
  async selectNode(): Promise<TeeSession> {
    const gatewayUrl = this.pickRandomNode();

    // ノード情報を取得してencryption_pubkeyを得る
    const nodeInfo = await this.getNodeInfo(gatewayUrl);

    // GlobalConfigからこのノードの情報を取得
    const teeNode = this.findTeeNodeBySigningPubkey(nodeInfo.signing_pubkey);

    return {
      gatewayUrl,
      encryptionPubkey: teeNode.encryption_pubkey,
      signingPubkey: teeNode.signing_pubkey,
    };
  }

  /**
   * Gateway の /.well-known/title-node-info を取得する。
   * 仕様書 §6.2
   */
  async getNodeInfo(gatewayUrl: string): Promise<NodeInfo> {
    const url = new URL("/.well-known/title-node-info", stripQuery(gatewayUrl));
    // APIキーが元URLにある場合は付与
    const apiKey = extractApiKey(gatewayUrl);
    if (apiKey) {
      url.searchParams.set("apikey", apiKey);
    }

    const res = await fetch(url.toString());
    if (!res.ok) {
      throw new Error(
        `ノード情報の取得に失敗: HTTP ${res.status} ${await res.text()}`
      );
    }
    return (await res.json()) as NodeInfo;
  }

  /**
   * Temporary Storageに暗号化ペイロードをアップロードするための署名付きURLを取得する。
   * 仕様書 §6.2 POST /upload-url
   */
  async getUploadUrl(
    gatewayUrl: string,
    contentSize: number,
    contentType: string
  ): Promise<{ uploadUrl: string; downloadUrl: string; expiresAt: number }> {
    const res = await this.gatewayPost(gatewayUrl, "/upload-url", {
      content_size: contentSize,
      content_type: contentType,
    });
    return {
      uploadUrl: res.upload_url,
      downloadUrl: res.download_url,
      expiresAt: res.expires_at,
    };
  }

  /**
   * 暗号化ペイロードをTemporary Storageにアップロードする。
   * 仕様書 §6.7
   *
   * @returns downloadUrl（TEEがフェッチするURL）
   */
  async upload(
    gatewayUrl: string,
    encryptedPayload: EncryptedPayload
  ): Promise<{ downloadUrl: string; sizeBytes: number }> {
    const payloadBytes = new TextEncoder().encode(
      JSON.stringify(encryptedPayload)
    );

    const { uploadUrl, downloadUrl } = await this.getUploadUrl(
      gatewayUrl,
      payloadBytes.length,
      "application/json"
    );

    // 署名付きURLにPUT
    const putRes = await fetch(uploadUrl, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: payloadBytes,
    });
    if (!putRes.ok) {
      throw new Error(
        `Temporary Storageへのアップロードに失敗: HTTP ${putRes.status}`
      );
    }

    return { downloadUrl, sizeBytes: payloadBytes.length };
  }

  /**
   * /verify エンドポイントを呼び出す。
   * 仕様書 §6.2
   * レスポンスはAES-GCM暗号化されている（EncryptedResponse）。
   */
  async verify(
    gatewayUrl: string,
    request: VerifyRequest
  ): Promise<{ nonce: string; ciphertext: string }> {
    const res = await this.gatewayPost(gatewayUrl, "/verify", request);
    return { nonce: res.nonce, ciphertext: res.ciphertext };
  }

  /**
   * /sign エンドポイントを呼び出す。
   * 仕様書 §6.2
   */
  async sign(
    gatewayUrl: string,
    request: SignRequest
  ): Promise<SignResponse> {
    return await this.gatewayPost(gatewayUrl, "/sign", request);
  }

  /**
   * /sign-and-mint エンドポイントを呼び出す（Gateway代行ミント）。
   * 仕様書 §6.2
   */
  async signAndMint(
    gatewayUrl: string,
    request: SignRequest
  ): Promise<{ txSignatures: string[] }> {
    const res = await this.gatewayPost(gatewayUrl, "/sign-and-mint", request);
    return { txSignatures: res.tx_signatures };
  }

  // --- GlobalConfig アクセス ---

  /**
   * GlobalConfigからtrusted_wasm_modulesを取得する。
   * 本番ではSolana RPCから直接読み取るが、現時点ではconfigから返す。
   * 仕様書 §5.2 Step 1
   */
  getTrustedWasmModules(): TrustedWasmModule[] {
    return this.config.globalConfig.trusted_wasm_modules;
  }

  /**
   * GlobalConfigからcore_collection_mintを取得する。
   * 仕様書 §5.2 Step 1
   */
  getCoreCollectionMint(): string {
    return this.config.globalConfig.core_collection_mint;
  }

  /**
   * GlobalConfigからext_collection_mintを取得する。
   * 仕様書 §5.2 Step 1
   */
  getExtCollectionMint(): string {
    return this.config.globalConfig.ext_collection_mint;
  }

  /**
   * GlobalConfigからtrusted_tee_nodesを取得する。
   * 仕様書 §5.2 Step 1
   */
  getTrustedTeeNodes(): TrustedTeeNode[] {
    return this.config.globalConfig.trusted_tee_nodes;
  }

  // --- 内部ヘルパー ---

  /** ランダムにTEEノードURLを選択する */
  private pickRandomNode(): string {
    const idx = Math.floor(Math.random() * this.config.teeNodes.length);
    return this.config.teeNodes[idx];
  }

  /** signing_pubkeyでGlobalConfig内のTeeNodeを検索する */
  private findTeeNodeBySigningPubkey(signingPubkey: string): TrustedTeeNode {
    const node = this.config.globalConfig.trusted_tee_nodes.find(
      (n) => n.signing_pubkey === signingPubkey
    );
    if (!node) {
      throw new Error(
        `GlobalConfigに signing_pubkey=${signingPubkey} のTEEノードが見つかりません`
      );
    }
    return node;
  }

  /** GatewayにPOSTリクエストを送信する */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private async gatewayPost(gatewayUrl: string, path: string, body: unknown): Promise<any> {
    const base = stripQuery(gatewayUrl);
    const url = new URL(path, base);
    const apiKey = extractApiKey(gatewayUrl);
    if (apiKey) {
      url.searchParams.set("apikey", apiKey);
    }

    const res = await fetch(url.toString(), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(`Gateway ${path} failed: HTTP ${res.status} - ${text}`);
    }

    return res.json();
  }
}

// ---------------------------------------------------------------------------
// ユーティリティ
// ---------------------------------------------------------------------------

/** URLからクエリパラメータを除去してベースURLを返す */
function stripQuery(url: string): string {
  const u = new URL(url);
  u.search = "";
  return u.toString().replace(/\/$/, "");
}

/** URLからapikeyクエリパラメータを抽出する */
function extractApiKey(url: string): string | null {
  try {
    const u = new URL(url);
    return u.searchParams.get("apikey");
  } catch {
    return null;
  }
}
