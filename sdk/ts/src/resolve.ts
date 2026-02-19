/**
 * resolve() 関数
 *
 * 仕様書 §1.2, §5.2: content_hashに対応するcNFTと来歴グラフを解決する。
 *
 * 内部処理フロー:
 * 1. content_hashに対応する権利トークンをDAS APIで検索
 * 2. cNFTのコレクション所属を確認（Global Config参照）
 * 3. オフチェーンデータを取得
 * 4. TEE署名を検証
 * 5. 来歴グラフの各ノードについて所有者を解決
 */

import type { ResolveResult } from "./types";

/**
 * content_hashからcNFTと来歴グラフを解決する。
 * 仕様書 §1.2, §5.2
 *
 * @param contentHash - SHA-256(Active Manifestの署名) のhex文字列
 */
export async function resolve(
  _contentHash: string
): Promise<ResolveResult> {
  // TODO: 1. DAS APIでcontent_hashに対応するcNFTを検索
  // TODO: 2. コレクション所属の確認（Global Config参照）
  // TODO: 3. 重複解決（仕様書 §2.4）
  // TODO: 4. オフチェーンデータの取得（json_uri）
  // TODO: 5. TEE署名の検証
  // TODO: 6. content_hashの一致確認
  // TODO: 7. 来歴グラフの解決（各ノードの所有者を特定）

  throw new Error("Not implemented");
}
