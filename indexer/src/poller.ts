/**
 * DAS API ポーラー
 *
 * 仕様書 §6.6: Webhookの欠落を補完するため、定期的にDAS APIをポーリングする。
 */

/**
 * DAS APIをポーリングし、未検知のイベントを検出する。
 * 仕様書 §6.6
 *
 * @param collectionMint - 監視対象のコレクションMintアドレス
 * @param lastCheckedSlot - 前回チェック時のスロット番号
 */
export async function pollDasApi(
  _collectionMint: string,
  _lastCheckedSlot: number
): Promise<void> {
  // TODO: DAS APIで最新のcNFTを取得
  // TODO: DBに存在しないcNFTを検出して挿入
  // TODO: Burn済みcNFTの検出と更新
  throw new Error("Not implemented");
}
