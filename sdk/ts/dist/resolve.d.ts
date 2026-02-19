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
export declare function resolve(_contentHash: string): Promise<ResolveResult>;
