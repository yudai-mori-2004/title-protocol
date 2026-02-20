/**
 * Webhook ハンドラ
 *
 * 仕様書 §6.6: Mint/Burn/Transferイベントをリアルタイムに検知してDBに反映する。
 * Helius Webhooks等のサービスからイベントを受信する。
 */

import type { IndexerDb } from "./db/client";
import type { DasClient } from "./das";

/** Webhookイベントの型 */
export interface WebhookEvent {
  type: "MINT" | "BURN" | "TRANSFER";
  assetId: string;
  owner: string;
  collection: string;
  timestamp: number;
}

/** signed_jsonの内容（オフチェーンメタデータ） */
interface OffchainMetadata {
  protocol?: string;
  payload?: {
    content_hash?: string;
    content_type?: string;
    creator_wallet?: string;
    extension_id?: string;
    tsa_timestamp?: number;
  };
  attributes?: Array<{
    trait_type: string;
    value: string;
  }>;
}

/**
 * Webhookイベントを処理する。
 * 仕様書 §6.6
 *
 * @param db - データベースクライアント
 * @param dasClient - DAS APIクライアント（MINTイベント時にメタデータ取得に使用）
 * @param event - Webhookイベント
 */
export async function handleWebhookEvent(
  db: IndexerDb,
  dasClient: DasClient,
  event: WebhookEvent
): Promise<void> {
  switch (event.type) {
    case "MINT":
      await handleMint(db, dasClient, event);
      break;

    case "BURN":
      await handleBurn(db, event);
      break;

    case "TRANSFER":
      await handleTransfer(db, event);
      break;
  }
}

/**
 * MINTイベント: DAS APIでアセット情報を取得し、オフチェーンデータをフェッチしてDBに挿入する。
 * 仕様書 §6.6
 */
async function handleMint(
  db: IndexerDb,
  dasClient: DasClient,
  event: WebhookEvent
): Promise<void> {
  // DAS APIでアセット情報を取得
  const asset = await dasClient.getAsset(event.assetId);

  // コレクションMintを取得
  const collectionGroup = asset.grouping.find((g) => g.group_key === "collection");
  const collectionMint = collectionGroup?.group_value ?? event.collection;

  // オフチェーンメタデータのURIを取得
  const jsonUri = asset.content.json_uri;
  if (!jsonUri) return;

  // オフチェーンデータをフェッチ
  const metadata = await fetchOffchainMetadata(jsonUri);
  if (!metadata?.payload?.content_hash) return;

  const payload = metadata.payload;
  const contentHash = payload.content_hash!;

  // CoreかExtensionかをprotocolフィールドで判定
  if (metadata.protocol === "Title-Extension-v1" && payload.extension_id) {
    // Extension cNFT
    await db.insertExtensionRecord({
      asset_id: event.assetId,
      content_hash: contentHash,
      extension_id: payload.extension_id,
      owner: asset.ownership.owner,
      signed_json_uri: jsonUri,
      collection_mint: collectionMint,
      solana_block_time: event.timestamp,
    });
  } else {
    // Core cNFT
    await db.insertCoreRecord({
      asset_id: event.assetId,
      content_hash: contentHash,
      content_type: payload.content_type ?? "application/octet-stream",
      owner: asset.ownership.owner,
      creator_wallet: payload.creator_wallet ?? asset.ownership.owner,
      signed_json_uri: jsonUri,
      collection_mint: collectionMint,
      tsa_timestamp: payload.tsa_timestamp ?? null,
      solana_block_time: event.timestamp,
    });
  }
}

/**
 * BURNイベント: cNFTをBurn済みとしてマークする。
 * 仕様書 §6.6
 */
async function handleBurn(db: IndexerDb, event: WebhookEvent): Promise<void> {
  await db.markBurned(event.assetId);
}

/**
 * TRANSFERイベント: 所有者を更新する。
 * 仕様書 §6.6
 */
async function handleTransfer(db: IndexerDb, event: WebhookEvent): Promise<void> {
  await db.updateOwner(event.assetId, event.owner);
}

/**
 * オフチェーンメタデータ（signed_json）をフェッチする。
 *
 * @param uri - メタデータのURI（Arweave等）
 */
async function fetchOffchainMetadata(uri: string): Promise<OffchainMetadata | null> {
  try {
    const res = await fetch(uri);
    if (!res.ok) return null;
    return (await res.json()) as OffchainMetadata;
  } catch {
    return null;
  }
}
