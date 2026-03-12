# @title-protocol/sdk

TypeScript SDK for [Title Protocol](https://github.com/yudai-mori-2004/title-protocol) — the identity layer for digital content.

Records digital content attribution on Solana using C2PA provenance, Trusted Execution Environments (TEE), and compressed NFTs.

## Install

```bash
npm install @title-protocol/sdk
```

## Quick Start

```typescript
import { fetchGlobalConfig, TitleClient } from "@title-protocol/sdk";

// 1. Fetch on-chain config (all data comes from Solana RPC)
const config = await fetchGlobalConfig("devnet");

// 2. Create client
const client = new TitleClient(config);

// 3. Register content (encrypt → upload → verify → store → sign)
const result = await client.register({
  content: imageBuffer,             // Uint8Array — C2PA-signed content
  ownerWallet: "YourSolana...",     // Base58 wallet address
  processorIds: ["core-c2pa"],      // processors to run in TEE
  storeSignedJson: async (json) => {
    // Persist signed_json to permanent storage (e.g. Arweave via Irys).
    // Return a retrievable URI.
    return await uploadToArweave(json);
  },
  recentBlockhash: blockhash,       // from connection.getLatestBlockhash()
});

// result.contents — verified content details (contentHash, storageUri, signedJson)
// result.partialTxs — Base64 partial TXs to co-sign with your wallet and broadcast
```

### Custom RPC

```typescript
import { Connection } from "@solana/web3.js";
import { fetchGlobalConfig, TitleClient } from "@title-protocol/sdk";

const conn = new Connection("https://devnet.helius-rpc.com/?api-key=...");
const config = await fetchGlobalConfig(conn, "devnet");
const client = new TitleClient(config);
```

### Delegate Minting

```typescript
const result = await client.register({
  content: imageBuffer,
  ownerWallet: wallet,
  storeSignedJson: myUploader,
  delegateMint: true,   // Gateway broadcasts the TX
});
// result.txSignatures — already on-chain
```

## API

### fetchGlobalConfig

Fetch GlobalConfig + all TeeNodeAccount PDAs from Solana. No HTTP requests — purely on-chain data.

```typescript
fetchGlobalConfig("devnet")                    // default RPC
fetchGlobalConfig(connection, "devnet")        // custom RPC
fetchGlobalConfig(connection, programId)       // custom program (node operators)
```

### TitleClient

```typescript
const client = new TitleClient(globalConfig);
```

| Method | Description |
|--------|-------------|
| `register(options)` | Full registration flow: encrypt → upload → verify → store → sign |
| `selectNode()` | Select a healthy TEE node (health-check + random selection) |

#### RegisterOptions

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `content` | `Uint8Array` | Yes | Content binary (C2PA-signed image, etc.) |
| `ownerWallet` | `string` | Yes | Owner wallet address (Base58) |
| `storeSignedJson` | `(json: string) => Promise<string>` | Yes | Callback to persist signed_json. Returns a URI. |
| `processorIds` | `string[]` | No | Processors to run. Default: `["core-c2pa"]` |
| `extensionInputs` | `Record<string, unknown>` | No | Auxiliary inputs for WASM extensions |
| `node` | `TeeSession` | No | Specific node to use (auto-selected if omitted) |
| `delegateMint` | `boolean` | No | If true, Gateway broadcasts TX. Default: false |
| `recentBlockhash` | `string` | No | Required when `delegateMint` is false |

### Low-level Methods

For advanced use cases, the underlying Gateway endpoints are available:

| Method | Description |
|--------|-------------|
| `getUploadUrl(url, size, type)` | Get a presigned upload URL |
| `upload(url, payload)` | Upload encrypted payload to temporary storage |
| `verifyRaw(url, request)` | Call `/verify` (returns encrypted response) |
| `signRaw(url, request)` | Call `/sign` |
| `signAndMintRaw(url, request)` | Call `/sign-and-mint` |

### Crypto

| Function | Description |
|----------|-------------|
| `encryptPayload(teePk, data)` | Full E2EE: ECDH + HKDF + AES-256-GCM |
| `decryptResponse(key, nonce, ct)` | Decrypt Base64-encoded TEE response |
| `generateEphemeralKeyPair()` | Generate X25519 keypair |
| `deriveSharedSecret(sk, pk)` | X25519 ECDH |
| `deriveSymmetricKey(shared)` | HKDF-SHA256 → 32-byte AES key |
| `encrypt(key, plaintext)` | AES-256-GCM encrypt |
| `decrypt(key, nonce, ct)` | AES-256-GCM decrypt |

## Security

The SDK automatically validates TEE responses inside `register()`:

- **wasm_hash check**: Extension signed_json's `wasm_hash` is verified against GlobalConfig's `trusted_wasm_modules`
- **These checks can also be performed manually** by reading the on-chain GlobalConfig directly

## Encryption Protocol

All content is end-to-end encrypted — node operators cannot see raw content:

1. Generate ephemeral X25519 keypair
2. ECDH with TEE's X25519 public key → shared secret
3. HKDF-SHA256 → 32-byte symmetric key
4. AES-256-GCM encrypt payload

The TEE encrypts its response with the same symmetric key.

## License

Apache-2.0
