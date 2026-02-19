/**
 * discoverNodes() 関数
 *
 * 仕様書 §6.7: Global Configから信頼されたTEEノードの一覧を取得し、
 * 各ノードのGatewayエンドポイントからスペック情報を収集する。
 */

import type { TrustedTeeNode, NodeInfo } from "./types";

/** ノード検索オプション */
export interface DiscoverOptions {
  /** ステータスフィルタ */
  status?: string;
  /** 最小コンテンツサイズ（バイト） */
  minSingleContentBytes?: number;
}

/** 検索結果のノード情報 */
export interface TeeNodeInfo extends TrustedTeeNode {
  /** /.well-known/title-node-info から取得したスペック情報 */
  nodeInfo: NodeInfo;
}

/**
 * 利用可能なTEEノードを検索する。
 * 仕様書 §6.7
 *
 * 1. Global Config から trusted_tee_nodes（status: Active）を取得
 * 2. 各ノードの gateway_endpoint/.well-known/title-node-info にアクセス
 * 3. オプション条件でフィルタリング
 */
export async function discoverNodes(
  _options?: DiscoverOptions
): Promise<TeeNodeInfo[]> {
  // TODO: 1. Solana RPCからGlobal Configを取得
  // TODO: 2. trusted_tee_nodesをフィルタリング
  // TODO: 3. 各ノードの /.well-known/title-node-info を取得
  // TODO: 4. オプション条件でフィルタリング

  throw new Error("Not implemented");
}
