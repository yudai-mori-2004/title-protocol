/**
 * register() 関数
 *
 * 仕様書 §6.7: コンテンツの検証・メタデータ保存・cNFT発行を実行する。
 *
 * 内部処理フロー:
 * 1. エフェメラルキーペア生成、ペイロード暗号化、Temporary Storageへアップロード
 * 2. /verify 呼び出し
 * 3. 暗号化レスポンスをエフェメラル秘密鍵で復号
 * 4. wasm_hash検証（セキュリティクリティカル）
 * 5. signed_jsonをオフチェーンストレージにアップロード
 * 6. /sign 呼び出し
 * 7. partial_txの検証、ウォレット署名、ブロードキャスト
 */
import type { RegisterResult, TrustedTeeNode } from "./types";
/** register() のオプション */
export interface RegisterOptions {
    /** コンテンツバイナリ */
    content: Uint8Array;
    /** コンテンツのMIMEタイプ */
    contentType: string;
    /** ウォレットアダプタ */
    owner: {
        publicKey: {
            toBase58(): string;
        };
        signTransaction(tx: unknown): Promise<unknown>;
    };
    /** 対象TEEノード */
    targetNode: TrustedTeeNode;
    /** 実行するプロセッサIDリスト */
    processorIds: string[];
    /** Extension補助入力（Optional） */
    extensionInputs?: Record<string, unknown>;
    /** Gateway代行ミントを使用するか */
    delegateMint?: boolean;
    /** サイドカーマニフェスト（Optional） */
    sidecarManifest?: Uint8Array;
}
/**
 * コンテンツの登録を実行する。
 * 仕様書 §6.7
 */
export declare function register(_options: RegisterOptions): Promise<RegisterResult>;
