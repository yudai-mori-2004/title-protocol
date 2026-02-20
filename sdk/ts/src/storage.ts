/**
 * オフチェーンストレージ
 *
 * 仕様書 §5.1 Step 7: signed_jsonをオフチェーンストレージにアップロードする。
 * CoreはArweave（永続保存が必須）、Extensionは任意のストレージを使用可能。
 */

/** ストレージプロバイダのインターフェース */
export interface StorageProvider {
  /**
   * データをアップロードし、永続的なURIを返す。
   *
   * @param data - アップロードするデータ（signed_jsonのJSON文字列）
   * @param contentType - MIMEタイプ
   * @returns URI (例: "ar://abc123...")
   */
  upload(data: Uint8Array, contentType: string): Promise<string>;
}

/**
 * Arweave (Irys経由) ストレージプロバイダ。Core用。
 * 仕様書 §5.1 Step 7
 *
 * Irys SDKを使用してArweaveにアップロードする。
 * Irys SDKは外部依存が重いため、コンストラクタでupload関数を注入する設計。
 */
export class ArweaveStorage implements StorageProvider {
  private _uploadFn: (data: Uint8Array, contentType: string) => Promise<string>;

  /**
   * @param uploadFn - 実際のアップロード処理。Irys SDKのupload呼び出しをラップする。
   *
   * 使用例:
   * ```ts
   * import Irys from "@irys/sdk";
   * const irys = new Irys({ url: "https://node2.irys.xyz", token: "solana", key: wallet });
   * const storage = new ArweaveStorage(async (data, contentType) => {
   *   const receipt = await irys.upload(Buffer.from(data), { tags: [{ name: "Content-Type", value: contentType }] });
   *   return `ar://${receipt.id}`;
   * });
   * ```
   */
  constructor(
    uploadFn: (data: Uint8Array, contentType: string) => Promise<string>
  ) {
    this._uploadFn = uploadFn;
  }

  async upload(data: Uint8Array, contentType: string): Promise<string> {
    return this._uploadFn(data, contentType);
  }
}

/**
 * HTTPベースのストレージプロバイダ。テストや開発用。
 * 指定URLにPOSTしてURIを受け取る。
 */
export class HttpStorage implements StorageProvider {
  constructor(private _endpoint: string) {}

  async upload(data: Uint8Array, contentType: string): Promise<string> {
    const res = await fetch(this._endpoint, {
      method: "POST",
      headers: { "Content-Type": contentType },
      body: data,
    });
    if (!res.ok) {
      throw new Error(
        `ストレージアップロードに失敗: HTTP ${res.status}`
      );
    }
    const result = (await res.json()) as { uri: string };
    return result.uri;
  }
}
