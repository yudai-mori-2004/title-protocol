"use strict";
/**
 * DB スキーマ定義
 *
 * 仕様書 §6.6: cNFTインデクサのデータベーススキーマ。
 */
Object.defineProperty(exports, "__esModule", { value: true });
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
