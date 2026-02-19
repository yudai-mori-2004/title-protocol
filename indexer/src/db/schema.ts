/**
 * DB スキーマ定義
 *
 * 仕様書 §6.6: cNFTインデクサのデータベーススキーマ。
 */

/** Core cNFTのテーブル定義 */
export interface CoreRecord {
  /** cNFTの一意識別子 (DAS API assetId) */
  asset_id: string;
  /** コンテンツハッシュ (SHA-256(Active Manifestの署名)) */
  content_hash: string;
  /** コンテンツのMIMEタイプ */
  content_type: string;
  /** 現在の所有者ウォレットアドレス (Base58) */
  owner: string;
  /** cNFTの作成者ウォレットアドレス (Base58) */
  creator_wallet: string;
  /** オフチェーンデータのURI */
  signed_json_uri: string;
  /** コレクションMintアドレス */
  collection_mint: string;
  /** TSAタイムスタンプ（存在する場合） */
  tsa_timestamp: number | null;
  /** Solana block time */
  solana_block_time: number;
  /** Burn済みかどうか */
  is_burned: boolean;
  /** レコード作成日時 */
  created_at: Date;
  /** レコード更新日時 */
  updated_at: Date;
}

/** Extension cNFTのテーブル定義 */
export interface ExtensionRecord {
  /** cNFTの一意識別子 */
  asset_id: string;
  /** コンテンツハッシュ */
  content_hash: string;
  /** Extension識別子 */
  extension_id: string;
  /** 現在の所有者ウォレットアドレス */
  owner: string;
  /** オフチェーンデータのURI */
  signed_json_uri: string;
  /** コレクションMintアドレス */
  collection_mint: string;
  /** Solana block time */
  solana_block_time: number;
  /** Burn済みかどうか */
  is_burned: boolean;
  /** レコード作成日時 */
  created_at: Date;
  /** レコード更新日時 */
  updated_at: Date;
}

/**
 * SQL DDL for reference:
 *
 * CREATE TABLE core_cnfts (
 *   asset_id TEXT PRIMARY KEY,
 *   content_hash TEXT NOT NULL,
 *   content_type TEXT NOT NULL,
 *   owner TEXT NOT NULL,
 *   creator_wallet TEXT NOT NULL,
 *   signed_json_uri TEXT NOT NULL,
 *   collection_mint TEXT NOT NULL,
 *   tsa_timestamp BIGINT,
 *   solana_block_time BIGINT NOT NULL,
 *   is_burned BOOLEAN NOT NULL DEFAULT FALSE,
 *   created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
 *   updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
 * );
 *
 * CREATE INDEX idx_core_content_hash ON core_cnfts(content_hash);
 * CREATE INDEX idx_core_owner ON core_cnfts(owner);
 *
 * CREATE TABLE extension_cnfts (
 *   asset_id TEXT PRIMARY KEY,
 *   content_hash TEXT NOT NULL,
 *   extension_id TEXT NOT NULL,
 *   owner TEXT NOT NULL,
 *   signed_json_uri TEXT NOT NULL,
 *   collection_mint TEXT NOT NULL,
 *   solana_block_time BIGINT NOT NULL,
 *   is_burned BOOLEAN NOT NULL DEFAULT FALSE,
 *   created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
 *   updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
 * );
 *
 * CREATE INDEX idx_ext_content_hash ON extension_cnfts(content_hash);
 * CREATE INDEX idx_ext_extension_id ON extension_cnfts(extension_id);
 * CREATE INDEX idx_ext_content_extension ON extension_cnfts(content_hash, extension_id);
 */
