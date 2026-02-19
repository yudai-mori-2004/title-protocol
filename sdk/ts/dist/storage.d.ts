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
export declare class ArweaveStorage implements StorageProvider {
    private _gateway;
    private _token;
    constructor(_gateway?: string, _token?: string);
    upload(_data: Uint8Array, _contentType: string): Promise<string>;
}
