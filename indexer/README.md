# @title-protocol/indexer

cNFT indexer for [Title Protocol](https://github.com/title-protocol/title-protocol) — indexes compressed NFTs minted by Title Protocol TEE nodes.

## Overview

The indexer monitors Solana cNFT collections via the [DAS (Digital Asset Standard) API](https://docs.helius.dev/compression-and-das-api/digital-asset-standard-das-api) and stores records in PostgreSQL. It supports two ingestion modes:

- **Webhook**: Receives real-time mint/burn/transfer events via HTTP POST
- **Poller**: Periodically scans collections to catch missed events

## Install

```bash
npm install @title-protocol/indexer
```

## Standalone Usage

```bash
# Required environment variables
export DATABASE_URL="postgres://user:pass@localhost:5432/title_indexer"
export DAS_ENDPOINTS="https://devnet.helius-rpc.com/?api-key=YOUR_KEY"
export COLLECTION_MINTS="CoreCollectionMint,ExtCollectionMint"

# Optional
export POLL_INTERVAL_MS=300000   # default: 5 minutes
export WEBHOOK_PORT=5000         # default: 5000

npm start
```

The process starts a webhook HTTP server and a background poller.

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/webhook` | Receive cNFT events (mint, burn, transfer) |
| `GET` | `/health` | Health check |

## Library Usage

```typescript
import { IndexerDb, DasClient } from "@title-protocol/indexer";
import type { CoreRecord, ExtensionRecord } from "@title-protocol/indexer";

// PostgreSQL client
const db = new IndexerDb("postgres://user:pass@localhost:5432/title_indexer");
await db.migrate();

// Query by content hash
const cores: CoreRecord[] = await db.findCoreByContentHash("sha256-hex");
const exts: ExtensionRecord[] = await db.findExtensionsByContentHash("sha256-hex");

// Query by owner wallet
const owned: CoreRecord[] = await db.findCoreByOwner("WalletAddress...");

// DAS API client (random endpoint selection)
const das = new DasClient(["https://devnet.helius-rpc.com/?api-key=KEY"]);
const assets = await das.getAllAssetsInCollection("CollectionMint...");

await db.close();
```

## API

### IndexerDb

| Method | Description |
|--------|-------------|
| `migrate()` | Create tables (idempotent) |
| `insertCoreRecord(record)` | Insert a Core cNFT record |
| `insertExtensionRecord(record)` | Insert an Extension cNFT record |
| `findCoreByContentHash(hash)` | Find Core cNFTs by content hash |
| `findCoreByOwner(owner)` | Find Core cNFTs by owner wallet |
| `getCoreByAssetId(id)` | Get a single Core cNFT by asset ID |
| `findExtensionsByContentHash(hash)` | Find Extension cNFTs by content hash |
| `findExtension(hash, extensionId)` | Find Extension cNFTs by content hash + extension ID |
| `markBurned(assetId)` | Mark a cNFT as burned |
| `updateOwner(assetId, newOwner)` | Update the owner of a cNFT |
| `getAllAssetIds()` | Get all known asset IDs (for poller diff detection) |

### DasClient

| Method | Description |
|--------|-------------|
| `getAssetsByGroup(mint, page, limit)` | Fetch cNFTs in a collection (paginated) |
| `getAllAssetsInCollection(mint)` | Fetch all cNFTs in a collection (auto-pagination) |
| `getAsset(assetId)` | Fetch a single asset by ID |

## Database Schema

Two tables are created by `migrate()`:

- **`core_cnfts`**: Core attribution records (content_hash, content_type, owner, creator_wallet, tsa_timestamp, etc.)
- **`extension_cnfts`**: Extension records (content_hash, extension_id, owner, etc.)

Both tables track `is_burned` and `updated_at` for lifecycle management.

## License

Apache-2.0
