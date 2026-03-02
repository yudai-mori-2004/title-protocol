# @title-protocol/sdk

TypeScript SDK for [Title Protocol](https://github.com/yudai-mori-2004/title-protocol) — the identity layer for digital content.

## Install

```bash
npm install @title-protocol/sdk
```

## Quick Start

```typescript
import {
  TitleClient,
  encryptPayload,
  decryptResponse,
} from "@title-protocol/sdk";

// 1. Initialize client
const client = new TitleClient({
  teeNodes: ["https://gateway.example.com"],
  solanaRpcUrl: "https://api.devnet.solana.com",
  globalConfig: { /* fetched from on-chain GlobalConfig PDA */ },
});

// 2. Select a TEE node (resolved from on-chain GlobalConfig)
const session = await client.selectNode();

// 3. Encrypt content with TEE's public key (E2EE)
const teePubkeyBytes = Buffer.from(session.encryptionPubkey, "base64");
const payload = JSON.stringify({
  owner_wallet: "YourSolanaWallet...",
  content: contentBase64,
});
const { symmetricKey, encryptedPayload } = await encryptPayload(
  teePubkeyBytes,
  new TextEncoder().encode(payload),
);

// 4. Upload encrypted payload
const { downloadUrl } = await client.upload(
  session.gatewayUrl,
  encryptedPayload,
);

// 5. Verify (C2PA verification + provenance graph)
const encrypted = await client.verify(session.gatewayUrl, {
  download_url: downloadUrl,
  processor_ids: ["core-c2pa", "phash-v1"],
});

// 6. Decrypt and inspect the verify response
const resultBytes = await decryptResponse(
  symmetricKey,
  encrypted.nonce,
  encrypted.ciphertext,
);
const verifyResult = JSON.parse(new TextDecoder().decode(resultBytes));

// 7. Upload signed_json to off-chain storage, then call /sign
const signResponse = await client.sign(session.gatewayUrl, {
  recent_blockhash: blockhash,
  requests: [{ signed_json_uri: arweaveUri }],
});
// signResponse.partial_txs contains partially-signed transactions
// to be co-signed by the user's wallet and broadcast.
```

## API

### TitleClient

| Method | Description |
|--------|-------------|
| `selectNode()` | Select a random TEE node and start a session (async, validates node availability) |
| `getUploadUrl(url, size, type)` | Get a signed upload URL for temporary storage |
| `upload(url, payload)` | Upload an encrypted payload and get a download URL |
| `verify(url, request)` | Call `/verify` — C2PA verification + provenance graph |
| `sign(url, request)` | Call `/sign` — generate cNFT minting transactions |
| `signAndMint(url, request)` | Call `/sign-and-mint` — Gateway-assisted minting |
| `getTrustedWasmModules()` | Get trusted WASM modules from GlobalConfig |
| `getTrustedTeeNodes()` | Get trusted TEE nodes from GlobalConfig |
| `getCoreCollectionMint()` | Get Core collection mint from GlobalConfig |
| `getExtCollectionMint()` | Get Extension collection mint from GlobalConfig |

### Crypto

| Function | Description |
|----------|-------------|
| `generateEphemeralKeyPair()` | Generate X25519 keypair for E2EE |
| `deriveSharedSecret(sk, pk)` | X25519 ECDH key exchange |
| `deriveSymmetricKey(shared)` | HKDF-SHA256 key derivation (32-byte AES key) |
| `encrypt(key, plaintext)` | AES-256-GCM encryption |
| `decrypt(key, nonce, ct)` | AES-256-GCM decryption |
| `encryptPayload(teePk, data)` | Full E2EE flow (ECDH + HKDF + AES-GCM) |
| `decryptResponse(key, nonce, ct)` | Decrypt Base64-encoded TEE response |

## Encryption Protocol

The SDK uses end-to-end encryption (E2EE) so that node operators cannot see content:

1. Client generates an ephemeral X25519 keypair
2. Derives a shared secret with the TEE's X25519 public key (ECDH)
3. Derives a symmetric key via HKDF-SHA256
4. Encrypts the payload with AES-256-GCM

The same symmetric key is used by the TEE to encrypt the response, which
the client decrypts with `decryptResponse()`.

## License

Apache-2.0
