# @title-protocol/sdk

TypeScript SDK for [Title Protocol](https://github.com/title-protocol/title-protocol) — the identity layer for digital content.

## Install

```bash
npm install @title-protocol/sdk
```

## Quick Start

```typescript
import { TitleClient, generateEphemeralKeyPair, deriveSharedSecret } from "@title-protocol/sdk";

// 1. Initialize client
const client = new TitleClient({
  teeNodes: ["http://gateway.example.com:3000"],
  solanaRpcUrl: "https://api.devnet.solana.com",
  globalConfig: { /* fetched from on-chain GlobalConfig PDA */ },
});

// 2. Select a TEE node (establishes session affinity)
const session = await client.selectNode();

// 3. Encrypt content with TEE's public key (E2EE)
const { publicKey, secretKey } = generateEphemeralKeyPair();
const sharedSecret = deriveSharedSecret(secretKey, session.encryptionPubkey);
// ... encrypt payload with AES-256-GCM using sharedSecret

// 4. Upload encrypted payload
const { downloadUrl } = await client.upload(session.gatewayUrl, encryptedPayload);

// 5. Verify (C2PA verification + provenance graph)
const encryptedResponse = await client.verify(session.gatewayUrl, {
  encrypted_payload_url: downloadUrl,
  client_pubkey: publicKey,
});

// 6. Sign (generate cNFT minting transaction)
const signResponse = await client.sign(session.gatewayUrl, {
  requests: [{ signed_json: decryptedVerifyResult, wallet: "YOUR_WALLET" }],
});
```

## API

### TitleClient

| Method | Description |
|--------|-------------|
| `selectNode()` | Select a random TEE node and start a session |
| `getNodeInfo(url)` | Fetch `/.well-known/title-node-info` from a Gateway |
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
| `deriveSharedSecret(secret, pubkey)` | X25519 ECDH + HKDF-SHA256 key derivation |

## Encryption Protocol

The SDK uses end-to-end encryption (E2EE) so that node operators cannot see content:

1. Client generates an ephemeral X25519 keypair
2. Derives a shared secret with the TEE's X25519 public key (ECDH)
3. Derives a symmetric key via HKDF-SHA256
4. Encrypts the payload with AES-256-GCM

## License

Apache-2.0
