/**
 * DAS API ポーラー
 *
 * 仕様書 §6.6: Webhookの欠落を補完するため、定期的にDAS APIをポーリングする。
 */

import type { IndexerDb } from "./db/client";
import type { DasClient, DasAsset } from "./das";

/**
 * DAS APIをポーリングし、DBとの差分を同期する。
 * 仕様書 §6.6
 *
 * 1. DAS API (getAssetsByGroup) でコレクション内の全cNFTを取得
 * 2. DBに存在しないcNFTを検出して挿入
 * 3. Burn済みcNFTの検出と更新
 *
 * @param db - データベースクライアント
 * @param dasClient - DAS APIクライアント
 * @param collectionMint - 監視対象のコレクションMintアドレス
 */
export async function pollDasApi(
  db: IndexerDb,
  dasClient: DasClient,
  collectionMint: string
): Promise<{ inserted: number; burned: number }> {
  // 全アセットを取得
  const assets = await dasClient.getAllAssetsInCollection(collectionMint);

  // 既存のasset_idを取得
  const existingIds = await db.getAllAssetIds();

  let inserted = 0;
  let burned = 0;

  for (const asset of assets) {
    // Burn済みの検出
    if (asset.burnt) {
      if (existingIds.has(asset.id)) {
        await db.markBurned(asset.id);
        burned++;
      }
      continue;
    }

    // 新規アセットの検出
    if (!existingIds.has(asset.id)) {
      await syncAssetToDb(db, asset, collectionMint);
      inserted++;
    }
  }

  return { inserted, burned };
}

/**
 * 単一アセットをDBに同期する。
 * DAS APIのレスポンスからprotocolを判定し、Core/Extensionを振り分ける。
 */
async function syncAssetToDb(
  db: IndexerDb,
  asset: DasAsset,
  collectionMint: string
): Promise<void> {
  const jsonUri = asset.content.json_uri;
  if (!jsonUri) return;

  // オフチェーンメタデータをフェッチ
  let metadata: Record<string, unknown> | null = null;
  try {
    const res = await fetch(jsonUri);
    if (res.ok) {
      metadata = (await res.json()) as Record<string, unknown>;
    }
  } catch {
    // フェッチ失敗時はスキップ（次回ポーリングで再試行）
    return;
  }
  if (!metadata) return;

  const payload = metadata.payload as Record<string, unknown> | undefined;
  if (!payload?.content_hash) return;

  const blockTime = asset.slot ?? Math.floor(Date.now() / 1000);

  if (
    metadata.protocol === "Title-Extension-v1" &&
    typeof payload.extension_id === "string"
  ) {
    await db.insertExtensionRecord({
      asset_id: asset.id,
      content_hash: payload.content_hash as string,
      extension_id: payload.extension_id,
      owner: asset.ownership.owner,
      signed_json_uri: jsonUri,
      collection_mint: collectionMint,
      solana_block_time: blockTime,
    });
  } else {
    await db.insertCoreRecord({
      asset_id: asset.id,
      content_hash: payload.content_hash as string,
      content_type: (payload.content_type as string) ?? "application/octet-stream",
      owner: asset.ownership.owner,
      creator_wallet: (payload.creator_wallet as string) ?? asset.ownership.owner,
      signed_json_uri: jsonUri,
      collection_mint: collectionMint,
      tsa_timestamp: (payload.tsa_timestamp as number) ?? null,
      solana_block_time: blockTime,
    });
  }
}

/**
 * ポーラーを定期実行で開始する。
 * 仕様書 §6.6
 *
 * @param db - データベースクライアント
 * @param dasClient - DAS APIクライアント
 * @param collectionMints - 監視対象のコレクションMintアドレス一覧
 * @param intervalMs - ポーリング間隔（ミリ秒）。デフォルト: 5分
 * @returns clearIntervalで停止可能なタイマー
 */
export function startPoller(
  db: IndexerDb,
  dasClient: DasClient,
  collectionMints: string[],
  intervalMs: number = 5 * 60 * 1000
): NodeJS.Timeout {
  const tick = async () => {
    for (const mint of collectionMints) {
      try {
        const result = await pollDasApi(db, dasClient, mint);
        if (result.inserted > 0 || result.burned > 0) {
          console.log(
            `[poller] ${mint}: inserted=${result.inserted}, burned=${result.burned}`
          );
        }
      } catch (err) {
        console.error(`[poller] ${mint}: エラー`, err);
      }
    }
  };

  // 初回即実行
  tick();

  return setInterval(tick, intervalMs);
}
