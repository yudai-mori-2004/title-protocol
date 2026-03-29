# Tests

Integration and system-level test infrastructure for Title Protocol.

## Prerequisites

Tests require a running TEE node:

1. **Local node** — Follow [deploy/local/README.md](../deploy/local/README.md) to start Gateway + TEE with MockRuntime
2. **Remote node** — Set `TEST_GATEWAY_URL`

The target node must have all WASM modules registered on-chain, a valid GlobalConfig PDA, and signed_json storage configured (Irys for core-c2pa, S3 or equivalent for extensions).

| Variable | Default | Description |
|----------|---------|-------------|
| `TEST_GATEWAY_URL` | `http://localhost:3000` | Gateway endpoint |
| `TEST_SOLANA_RPC` | `https://api.devnet.solana.com` | Solana RPC |
| `TEST_PROGRAM_ID` | Built-in default | title-config program ID |

```bash
cd tests/integration
npm install
npm test                    # all tests
npm run test:core           # core-c2pa only
npm run test:extensions     # per-extension only
```

## Directory Structure

```
tests/
├── fixtures/              Shared test data (Rust and TS tests both reference this)
│   ├── certs/             Test CA + leaf cert/key for C2PA signing
│   ├── minimal/           Source files for factory (ffmpeg-generated)
│   ├── images/jpeg/       External: Google Pixel C2PA-signed (committed)
│   └── c2pa/              Factory-generated C2PA signed/unsigned pairs
│       ├── signed/        JPEG/PNG/TIFF/WAV/MP3/MP4 × sizes + ingredients
│       └── unsigned/      Same formats without C2PA manifests
│
├── factory/               Fixture generation
│                          `./tests/factory/generate.sh`
│
├── integration/           SDK → Gateway → TEE tests (TypeScript, node:test)
│   ├── helpers/           Test context, encrypt/verify, Ed25519 verification
│   ├── core/              core-c2pa: provenance graph, content_hash, TSA
│   ├── extensions/        Per-processor: breadth (formats) + depth (edge cases)
│   ├── security/          Crypto: signature verification, content_hash consistency
│   ├── concurrent/        Parallel requests, state isolation
│   ├── multi/             Multiple processors, all-or-nothing
│   └── errors/            Expected rejections with specific assertions
│
├── perf/                  Performance and load tests
│   └── stress-test.ts     Load, concurrency, malformed input, crypto attacks
│
└── cli/                   CLI working files (tee-info.json)
```

## Design Principles

### Single processor, single content = one test

Each processor (core-c2pa, image-pdq, cert-google, etc.) is tested independently. Multi-processor, concurrent, and security tests are separate, smaller layers.

### Three test dimensions per processor

1. **Breadth** — which content formats can this processor handle? Test every supported format with a C2PA-signed fixture.
2. **Depth** — edge cases specific to this processor. Examples: near-duplicate images (PDQ), wrong vendor cert chain (cert-*), 1-frame video (vPDQ), complex ingredient graph (core-c2pa).
3. **Determinism** — same input must produce the same output. Send the same file twice in one test run, assert identical content_hash and attribute values. Do not hardcode expected hashes against regenerable fixtures.

### All content must be C2PA-signed

The TEE derives content_hash from the C2PA manifest's COSE_Sign1 signature. Content without C2PA manifests is rejected by ALL processors. This means:

- Format breadth tests use factory-generated C2PA-signed files
- Unsigned fixtures are only for error-path tests

### Storage follows real-world routing

Tests use the Gateway's storage delegation (`signAndMint` without `storeSignedJson` callback). The Gateway routes:
- `core-c2pa` signed_json → Irys (Arweave)
- Extensions → S3 or default storage

This mirrors production usage and verifies the full pipeline including storage.

### Fixtures: factory vs external

| Type | Regenerable? | Used by | Examples |
|------|-------------|---------|---------|
| Factory | Yes | core-c2pa, image-pdq, video-vpdq | C2PA self-signed JPEG/TIFF/MP3/WAV |
| External | No (real hardware) | cert-google, cert-sony, cert-leica, cert-rootlens | pixel_*.jpg (Google Pixel) |
| Certs | Committed, stable | Factory input | Test CA chain |
| Minimal | Committed, stable | Factory input | 4x4 JPEG, 2x2 PNG, 1-sec MP4 |

**Factory fixtures use a self-signed test CA.** Cert-chain processors return `verified: false` for factory fixtures. Only external fixtures with real vendor signatures produce `verified: true`.

### Error tests must assert specific outcomes

```typescript
// BAD
try { await verify(unsigned, ["core-c2pa"]); } catch { /* ok */ }

// GOOD
await assert.rejects(
  () => verify(unsigned, ["core-c2pa"]),
  (err: Error) => err.message.includes("JUMBF")
);
```

### Every response gets validated

All test helpers must:
1. **Schema-validate** signed_json — required fields present, correct types
2. **Verify `tee_signature`** against `tee_pubkey` using Ed25519 — not just a truthy check, actual cryptographic verification
3. **Assert `content_type`** matches expected MIME type

## Test Categories

| Category | What it tests | Fixture type |
|----------|--------------|--------------|
| core/ | C2PA verification, provenance graph, content_hash determinism, TSA | Factory + external |
| extensions/ | Per-processor format breadth + depth, output field validation | Factory (hash) + external (cert) |
| multi/ | Multiple processors in one request, all-or-nothing failure (422) | Factory |
| errors/ | Unsigned content rejection, wrong processor for format | Unsigned, malformed |
| security/ | Wrong encryption key, tampered ciphertext, replayed nonce, stale TEE pubkey | Programmatic |
| concurrent/ | N parallel requests, no result mixing, resource contention | Factory |

### All-or-nothing

When multiple processors run and ANY fails, the entire request returns 422. Tests verify:
- `["core-c2pa", "cert-google"]` with non-Google content → 422
- `["core-c2pa", "video-vpdq"]` with image → 422
- Error identifies which processor failed

### Combinatorial strategy

Do NOT test all processor pairs (O(n^2)). Test:
1. Each extension paired with core-c2pa
2. Two hash processors together
3. Hash + cert together
4. All processors at once (one smoke test)

### Performance (tests/perf/)

1. **Baseline**: single request latency per processor (p50/p95/p99)
2. **Throughput**: concurrent requests, increasing parallelism → saturation point
3. **Scaling**: vary TEE specs (memory, vCPU) → proportional vs constant costs
4. **Content size**: vary input → transfer-bound vs compute-bound

## Adding a New Extension

1. Create `tests/integration/extensions/<name>.test.ts`
2. Add C2PA-signed fixtures for supported formats to `tests/fixtures/c2pa/signed/`
3. Register fixture paths in `tests/integration/helpers/fixtures.ts`
4. Add breadth tests (formats) and depth tests (edge cases)
5. If it needs real vendor fixtures (like cert-*), commit them and document source
6. Add one combination test in `multi/` pairing it with core-c2pa

## What will rot first

- **`fixtures.ts` path registry** — hardcoded paths. Consider glob-based discovery.
- **Factory output** — regeneration changes content_hash. Commit and treat as stable.
- **Default gateway URL** — must stay localhost. Remote testing via env var override.

## CI

`cargo test --workspace` and SDK unit tests run on every push. Integration tests require a live TEE node and run in a dedicated CI stage or manually.
