/**
 * DAS API クライアント
 *
 * 仕様書 §6.6: Helius DAS APIを使用してcNFTを取得する。
 *
 * DASエンドポイントはAPIキー付きURLのフラット配列で管理し、
 * リクエストごとにランダムで選択する（SDK側のTEEノード管理と同じパターン）。
 *
 * 例: ["https://mainnet.helius-rpc.com/?api-key=xxx", "https://devnet.helius-rpc.com/?api-key=yyy"]
 */

/** DAS APIで返されるcNFTアセット */
export interface DasAsset {
  id: string;
  ownership: {
    owner: string;
    delegate: string | null;
  };
  grouping: Array<{
    group_key: string;
    group_value: string;
  }>;
  content: {
    json_uri: string;
    metadata: {
      name: string;
      symbol: string;
      attributes?: Array<{
        trait_type: string;
        value: string;
      }>;
    };
  };
  burnt: boolean;
  slot?: number;
}

/** DAS API getAssetsByGroup レスポンス */
export interface DasGetAssetsByGroupResponse {
  total: number;
  limit: number;
  page: number;
  items: DasAsset[];
}

/** DAS API getAsset レスポンス */
export type DasGetAssetResponse = DasAsset;

/**
 * DASクライアント。
 *
 * 複数のDASエンドポイントをフラット配列で管理し、ランダムに選択する。
 */
export class DasClient {
  private endpoints: string[];

  /**
   * @param endpoints - DAS APIエンドポイントの配列（APIキー付きURL）
   *   例: ["https://mainnet.helius-rpc.com/?api-key=xxx"]
   */
  constructor(endpoints: string[]) {
    if (endpoints.length === 0) {
      throw new Error("DASエンドポイントが1つ以上必要です");
    }
    this.endpoints = endpoints;
  }

  /** ランダムにエンドポイントを選択する */
  private pickEndpoint(): string {
    return this.endpoints[Math.floor(Math.random() * this.endpoints.length)];
  }

  /** DAS JSON-RPCリクエストを送信する */
  private async rpc<T>(method: string, params: unknown): Promise<T> {
    const endpoint = this.pickEndpoint();
    const body = {
      jsonrpc: "2.0",
      id: 1,
      method,
      params,
    };

    const res = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      throw new Error(`DAS APIエラー: HTTP ${res.status}`);
    }

    const json = (await res.json()) as { result?: T; error?: { message: string } };
    if (json.error) {
      throw new Error(`DAS RPCエラー: ${json.error.message}`);
    }
    if (!json.result) {
      throw new Error("DAS APIからresultが返されませんでした");
    }
    return json.result;
  }

  /**
   * コレクション内のcNFTを取得する。
   * 仕様書 §6.6: getAssetsByGroupでコレクション内の全cNFTを取得。
   *
   * @param collectionMint - コレクションのMintアドレス
   * @param page - ページ番号（1始まり）
   * @param limit - 1ページあたりの件数
   */
  async getAssetsByGroup(
    collectionMint: string,
    page: number = 1,
    limit: number = 1000
  ): Promise<DasGetAssetsByGroupResponse> {
    return this.rpc<DasGetAssetsByGroupResponse>("getAssetsByGroup", {
      groupKey: "collection",
      groupValue: collectionMint,
      page,
      limit,
      sortBy: { sortBy: "created", sortDirection: "asc" },
    });
  }

  /**
   * 全ページを自動取得する。
   *
   * @param collectionMint - コレクションのMintアドレス
   */
  async getAllAssetsInCollection(collectionMint: string): Promise<DasAsset[]> {
    const all: DasAsset[] = [];
    let page = 1;
    const limit = 1000;

    while (true) {
      const response = await this.getAssetsByGroup(collectionMint, page, limit);
      all.push(...response.items);

      if (all.length >= response.total || response.items.length === 0) {
        break;
      }
      page++;
    }

    return all;
  }

  /**
   * 個別のアセットを取得する。
   *
   * @param assetId - アセットID
   */
  async getAsset(assetId: string): Promise<DasGetAssetResponse> {
    return this.rpc<DasGetAssetResponse>("getAsset", { id: assetId });
  }
}
