"use strict";
/**
 * discoverNodes() 関数
 *
 * 仕様書 §6.7: Global Configから信頼されたTEEノードの一覧を取得し、
 * 各ノードのGatewayエンドポイントからスペック情報を収集する。
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.discoverNodes = discoverNodes;
/**
 * 利用可能なTEEノードを検索する。
 * 仕様書 §6.7
 *
 * 1. Global Config から trusted_tee_nodes（status: Active）を取得
 * 2. 各ノードの gateway_endpoint/.well-known/title-node-info にアクセス
 * 3. オプション条件でフィルタリング
 */
async function discoverNodes(_options) {
    // TODO: 1. Solana RPCからGlobal Configを取得
    // TODO: 2. trusted_tee_nodesをフィルタリング
    // TODO: 3. 各ノードの /.well-known/title-node-info を取得
    // TODO: 4. オプション条件でフィルタリング
    throw new Error("Not implemented");
}
