# scripts/

Operational scripts for Title Protocol node management.

## Scripts

| Script | Description |
|--------|-------------|
| `init-devnet.mjs` | Full Devnet initialization: create collection, set GlobalConfig, register TEE, upload WASM modules, delegate authority, create Merkle Tree |
| `init-config.mjs` | GlobalConfig initialization helper for local development (subset of init-devnet) |
| `register-content.mjs` | Content registration CLI (end-to-end flow: upload, verify, sign, mint) |

## Usage

```bash
cd scripts
npm install

# Full Devnet initialization
node init-devnet.mjs --gateway http://<EC2_IP>:3000 [--rpc <SOLANA_RPC_URL>] [--skip-tree] [--skip-delegate]

# Local dev GlobalConfig setup (called by setup-local.sh)
node init-config.mjs --rpc http://localhost:8899 --gateway http://localhost:3000 --tee http://localhost:4000

# Register content (uses environment variables for endpoints)
GATEWAY_URL=http://<EC2_IP>:3000 \
SOLANA_RPC_URL=https://api.devnet.solana.com \
  node register-content.mjs <image.jpg> [--processor core-c2pa,phash-v1]
```

## Requirements

- Node.js 24+
- Solana CLI (for keypair management)
- Running Gateway and TEE server
