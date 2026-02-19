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

/** Arweave (Irys経由) ストレージプロバイダ。Core用。 */
export class ArweaveStorage implements StorageProvider {
  constructor(
    private _gateway: string = "https://node2.irys.xyz",
    private _token: string = "solana"
  ) {}

  async upload(
    _data: Uint8Array,
    _contentType: string
  ): Promise<string> {
    // TODO: Irys SDKを使用したArweaveアップロード
    throw new Error("Not implemented");
  }
}
