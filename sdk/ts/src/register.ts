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
  owner: { publicKey: { toBase58(): string }; signTransaction(tx: unknown): Promise<unknown> };
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
export async function register(
  _options: RegisterOptions
): Promise<RegisterResult> {
  // TODO: 1. エフェメラルキーペア生成
  // TODO: 2. ClientPayload構築
  // TODO: 3. ペイロード暗号化（ECDH → HKDF → AES-GCM）
  // TODO: 4. /upload-url で署名付きURL取得
  // TODO: 5. 暗号化ペイロードをTemporary Storageにアップロード
  // TODO: 6. /verify 呼び出し
  // TODO: 7. レスポンス復号
  // TODO: 8. wasm_hash検証（セキュリティクリティカル - 仕様書 §6.4）
  // TODO: 9. signed_jsonをオフチェーンストレージにアップロード
  // TODO: 10. /sign 呼び出し
  // TODO: 11. partial_tx検証、ウォレット署名、ブロードキャスト

  throw new Error("Not implemented");
}
