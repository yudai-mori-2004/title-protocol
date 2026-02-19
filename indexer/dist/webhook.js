"use strict";
/**
 * Webhook ハンドラ
 *
 * 仕様書 §6.6: Mint/Burnイベントをリアルタイムに検知してDBに反映する。
 * Helius Webhooks等のサービスからイベントを受信する。
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.handleWebhookEvent = handleWebhookEvent;
/**
 * Webhookイベントを処理する。
 * 仕様書 §6.6
 */
async function handleWebhookEvent(_event) {
    // TODO: イベントタイプに応じた処理
    //   MINT: 新規cNFTをDBに挿入
    //   BURN: cNFTをBurn済みとしてマーク
    //   TRANSFER: 所有者を更新
    throw new Error("Not implemented");
}
