/**
 * Webhook ハンドラ
 *
 * 仕様書 §6.6: Mint/Burnイベントをリアルタイムに検知してDBに反映する。
 * Helius Webhooks等のサービスからイベントを受信する。
 */

/** Webhookイベントの型 */
export interface WebhookEvent {
  type: "MINT" | "BURN" | "TRANSFER";
  assetId: string;
  owner: string;
  collection: string;
  timestamp: number;
}

/**
 * Webhookイベントを処理する。
 * 仕様書 §6.6
 */
export async function handleWebhookEvent(
  _event: WebhookEvent
): Promise<void> {
  // TODO: イベントタイプに応じた処理
  //   MINT: 新規cNFTをDBに挿入
  //   BURN: cNFTをBurn済みとしてマーク
  //   TRANSFER: 所有者を更新
  throw new Error("Not implemented");
}
