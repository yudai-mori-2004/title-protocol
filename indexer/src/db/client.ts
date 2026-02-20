/**
 * PostgreSQLクライアント
 *
 * 仕様書 §6.6: cNFTインデクサのCRUD操作。
 */

import { Pool, type PoolConfig } from "pg";
import type { CoreRecord, ExtensionRecord } from "./schema";

export class IndexerDb {
  private pool: Pool;

  constructor(config: PoolConfig | string) {
    this.pool =
      typeof config === "string" ? new Pool({ connectionString: config }) : new Pool(config);
  }

  /** 接続プールを閉じる */
  async close(): Promise<void> {
    await this.pool.end();
  }

  // ---------------------------------------------------------------------------
  // マイグレーション
  // ---------------------------------------------------------------------------

  /** テーブル作成（べき等）。仕様書 §6.6 */
  async migrate(): Promise<void> {
    await this.pool.query(`
      CREATE TABLE IF NOT EXISTS core_cnfts (
        asset_id TEXT PRIMARY KEY,
        content_hash TEXT NOT NULL,
        content_type TEXT NOT NULL,
        owner TEXT NOT NULL,
        creator_wallet TEXT NOT NULL,
        signed_json_uri TEXT NOT NULL,
        collection_mint TEXT NOT NULL,
        tsa_timestamp BIGINT,
        solana_block_time BIGINT NOT NULL,
        is_burned BOOLEAN NOT NULL DEFAULT FALSE,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
      );

      CREATE INDEX IF NOT EXISTS idx_core_content_hash ON core_cnfts(content_hash);
      CREATE INDEX IF NOT EXISTS idx_core_owner ON core_cnfts(owner);

      CREATE TABLE IF NOT EXISTS extension_cnfts (
        asset_id TEXT PRIMARY KEY,
        content_hash TEXT NOT NULL,
        extension_id TEXT NOT NULL,
        owner TEXT NOT NULL,
        signed_json_uri TEXT NOT NULL,
        collection_mint TEXT NOT NULL,
        solana_block_time BIGINT NOT NULL,
        is_burned BOOLEAN NOT NULL DEFAULT FALSE,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
      );

      CREATE INDEX IF NOT EXISTS idx_ext_content_hash ON extension_cnfts(content_hash);
      CREATE INDEX IF NOT EXISTS idx_ext_extension_id ON extension_cnfts(extension_id);
      CREATE INDEX IF NOT EXISTS idx_ext_content_extension ON extension_cnfts(content_hash, extension_id);
    `);
  }

  // ---------------------------------------------------------------------------
  // Core CRUD
  // ---------------------------------------------------------------------------

  /** Core cNFTを挿入する（ON CONFLICT無視）。仕様書 §6.6 */
  async insertCoreRecord(
    record: Omit<CoreRecord, "created_at" | "updated_at" | "is_burned">
  ): Promise<void> {
    await this.pool.query(
      `INSERT INTO core_cnfts
        (asset_id, content_hash, content_type, owner, creator_wallet, signed_json_uri, collection_mint, tsa_timestamp, solana_block_time)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
       ON CONFLICT (asset_id) DO NOTHING`,
      [
        record.asset_id,
        record.content_hash,
        record.content_type,
        record.owner,
        record.creator_wallet,
        record.signed_json_uri,
        record.collection_mint,
        record.tsa_timestamp,
        record.solana_block_time,
      ]
    );
  }

  /** content_hashでCore cNFTを検索する。仕様書 §6.6 */
  async findCoreByContentHash(contentHash: string): Promise<CoreRecord[]> {
    const result = await this.pool.query<CoreRecord>(
      `SELECT * FROM core_cnfts WHERE content_hash = $1 AND is_burned = FALSE ORDER BY solana_block_time ASC`,
      [contentHash]
    );
    return result.rows;
  }

  /** ownerでCore cNFTを検索する */
  async findCoreByOwner(owner: string): Promise<CoreRecord[]> {
    const result = await this.pool.query<CoreRecord>(
      `SELECT * FROM core_cnfts WHERE owner = $1 AND is_burned = FALSE ORDER BY solana_block_time DESC`,
      [owner]
    );
    return result.rows;
  }

  /** asset_idでCore cNFTを1件取得する */
  async getCoreByAssetId(assetId: string): Promise<CoreRecord | null> {
    const result = await this.pool.query<CoreRecord>(
      `SELECT * FROM core_cnfts WHERE asset_id = $1`,
      [assetId]
    );
    return result.rows[0] ?? null;
  }

  // ---------------------------------------------------------------------------
  // Extension CRUD
  // ---------------------------------------------------------------------------

  /** Extension cNFTを挿入する（ON CONFLICT無視）。仕様書 §6.6 */
  async insertExtensionRecord(
    record: Omit<ExtensionRecord, "created_at" | "updated_at" | "is_burned">
  ): Promise<void> {
    await this.pool.query(
      `INSERT INTO extension_cnfts
        (asset_id, content_hash, extension_id, owner, signed_json_uri, collection_mint, solana_block_time)
       VALUES ($1, $2, $3, $4, $5, $6, $7)
       ON CONFLICT (asset_id) DO NOTHING`,
      [
        record.asset_id,
        record.content_hash,
        record.extension_id,
        record.owner,
        record.signed_json_uri,
        record.collection_mint,
        record.solana_block_time,
      ]
    );
  }

  /** content_hashでExtension cNFTを検索する */
  async findExtensionsByContentHash(contentHash: string): Promise<ExtensionRecord[]> {
    const result = await this.pool.query<ExtensionRecord>(
      `SELECT * FROM extension_cnfts WHERE content_hash = $1 AND is_burned = FALSE ORDER BY solana_block_time ASC`,
      [contentHash]
    );
    return result.rows;
  }

  /** content_hash + extension_idで検索する */
  async findExtension(
    contentHash: string,
    extensionId: string
  ): Promise<ExtensionRecord[]> {
    const result = await this.pool.query<ExtensionRecord>(
      `SELECT * FROM extension_cnfts WHERE content_hash = $1 AND extension_id = $2 AND is_burned = FALSE`,
      [contentHash, extensionId]
    );
    return result.rows;
  }

  // ---------------------------------------------------------------------------
  // Burn / Transfer
  // ---------------------------------------------------------------------------

  /** cNFTをBurn済みとしてマークする。仕様書 §6.6 */
  async markBurned(assetId: string): Promise<void> {
    await this.pool.query(
      `UPDATE core_cnfts SET is_burned = TRUE, updated_at = NOW() WHERE asset_id = $1`,
      [assetId]
    );
    await this.pool.query(
      `UPDATE extension_cnfts SET is_burned = TRUE, updated_at = NOW() WHERE asset_id = $1`,
      [assetId]
    );
  }

  /** 所有者を更新する。仕様書 §6.6 */
  async updateOwner(assetId: string, newOwner: string): Promise<void> {
    await this.pool.query(
      `UPDATE core_cnfts SET owner = $1, updated_at = NOW() WHERE asset_id = $2`,
      [newOwner, assetId]
    );
    await this.pool.query(
      `UPDATE extension_cnfts SET owner = $1, updated_at = NOW() WHERE asset_id = $2`,
      [newOwner, assetId]
    );
  }

  /** 全asset_idの一覧を取得する（ポーラーの差分検出用） */
  async getAllAssetIds(): Promise<Set<string>> {
    const coreResult = await this.pool.query<{ asset_id: string }>(
      `SELECT asset_id FROM core_cnfts`
    );
    const extResult = await this.pool.query<{ asset_id: string }>(
      `SELECT asset_id FROM extension_cnfts`
    );
    const ids = new Set<string>();
    for (const row of coreResult.rows) ids.add(row.asset_id);
    for (const row of extResult.rows) ids.add(row.asset_id);
    return ids;
  }
}
