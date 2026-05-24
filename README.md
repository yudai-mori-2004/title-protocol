# Title Protocol

[![CI](https://github.com/yudai-mori-2004/title-protocol/actions/workflows/ci.yml/badge.svg)](https://github.com/yudai-mori-2004/title-protocol/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust: 1.93.1](https://img.shields.io/badge/rust-1.93.1-orange.svg)](rust-toolchain.toml)

**Attribute Extraction Layer for C2PA-signed Content**

> The technical specification is written in Japanese
> (`docs/v0.1.2/SPECS_JA.md`). Code, docstrings, commit messages, and PR
> review are in English; the JA spec is the source of truth for protocol
> design only.

## Quickstart

```bash
git clone https://github.com/yudai-mori-2004/title-protocol.git
cd title-protocol
docker compose up --build -d   # TEE (mock runtime) + Gateway
./docker/smoke-test.sh         # 5 endpoints, ~10s
```

> **NOTE:** This Quickstart starts the TEE with a **mock runtime** — it returns
> a fake Attestation Document that says "DO-NOT-APPROVE" and is unsafe for any
> production use. Hardware-backed (AWS Nitro Enclave) deployment is the only
> path that produces real, verifiable attestations; see
> [`deploy/aws/README.md`](deploy/aws/README.md).

---

## What It Does

Title Protocol extracts attributes from C2PA-signed digital content inside a TEE (Trusted Execution Environment) and seals the results with an Attestation Document.

The TEE's hardware guarantees that:
- The extraction code ran unmodified (verified by the Attestation Document's measurement)
- The results are untampered (verified by the hash in the Attestation Document's user_data)
- The content was never visible to the operator (hardware-level memory isolation)

This lets a third party verify that attributes were correctly extracted from authentic content — without needing the original content, and without trusting anyone except the TEE hardware.

## The Problem

C2PA provides cryptographic provenance for digital content, but has three structural constraints:

1. **Verification requires the full original file** — you need every byte to recompute the hash
2. **Verification exposes all metadata** — location, device info, edit history leak to the verifier
3. **Metadata is stripped in distribution** — social platforms and messaging apps delete C2PA manifests on upload

Title Protocol resolves all three by delegating C2PA verification to a TEE and sealing selected attributes into an Attestation Document that exists independently of the original content.

| | C2PA alone | Via Title Protocol |
|---|---|---|
| Verification input | Full original binary | Attestation Document + extracted attributes |
| Metadata exposure | Entire manifest | Only requested attributes |
| When platforms re-compress content | Embedded manifest is stripped | Output exists independently |

## How It Works

```
Client                              TEE
  |                                  |
  |  Content URL                     |
  |  Processor list                  |
  |                                  |
  |--------------------------------->|
  |                          Fetch content from URL
  |                          Run processors in parallel
  |                          Compute result hash
  |                          Get Attestation Document
  |                            (hash in user_data)
  |                                  |
  |<---------------------------------|
  |                                  |
  |  Results + Attestation Document  |
  |                                  |
```

1. Client sends a content URL and a list of processors to run
2. TEE fetches the content, verifies C2PA signatures, runs processors, and assembles results
3. TEE embeds the result hash into an Attestation Document and returns both

What happens after — storage, blockchain recording, access control — is outside the protocol's scope.

## Architecture

```
Client --> Gateway --> TEE --> External Storage (user-managed)
```

Two components. No intermediate storage managed by the protocol.

| Component | Role |
|---|---|
| **Gateway** | Client authentication, TEE info relay, request routing |
| **TEE** | Content fetch, C2PA verification, attribute extraction, Attestation Document generation |

### Processors

`c2pa-verify` runs on every request and produces the `signature_hash` — the protocol-level content identifier derived from the active manifest's signature. All other processors are optional.

| Processor | Output |
|---|---|
| `c2pa-verify` | C2PA signature chain validation, `signature_hash` |
| `provenance-graph` | Ingredient relationship DAG |
| `image-pdq` | PDQ 256-bit perceptual hash |
| `video-vpdq` | Per-frame vPDQ hash sequence |
| `cert-google` | Google C2PA root CA chain verification |
| `cert-sony` | Sony C2PA root CA chain verification |
| `cert-leica` | Leica C2PA root CA chain verification |

### Input Types

| Type | Use case |
|---|---|
| `single` | JPEG, PNG, MP4 (full-body fetch; HTTP Range Request streaming is on the roadmap) |
| `fragmented` | CMAF streaming segments (init.mp4 + seg-*.m4s) |
| `sidecar` | Detached C2PA manifest (.c2pa) + content file |

### Encryption (Optional)

Client-to-TEE encryption is available when content confidentiality is needed. Three suites: X25519, P-256, ML-KEM-768 (post-quantum). When omitted, content is processed as plaintext over HTTPS.

### Extension Layer

Extensions consume the core output for domain-specific purposes. The initial extension is **Solana Extension**, which records results on-chain as cNFTs with a ZK-proven TEE signing key whitelist.

## Trust Model

One assumption: **the TEE hardware works as specified** (attestation measurements are honest).

Given this:
- Measurement matches the published source code build hash → the correct program ran
- user_data hash matches the results → the results are untampered
- TEE memory isolation → the operator never saw the content

No trust in the Gateway, storage provider, or protocol operator is required.

## Design Principles

| Principle | Description |
|---|---|
| **Content-Agnostic** | Raw content is processed only inside the TEE; the operator cannot see it |
| **Stateless** | No state between requests; keys are ephemeral and lost on restart |
| **Neutral** | Not tied to any specific application, storage, or blockchain |
| **E2EE Optional** | Encryption is available but not forced; plaintext over HTTPS is valid |

## Status

**v0.1.2 — Core implementation complete; AWS Nitro verification ongoing.**

Gateway, TEE, Solana Extension, and SP1 attestation guest are all implemented
and exercised end-to-end on devnet. Remaining work tracked in
[`docs/v0.1.2/COVERAGE.md`](docs/v0.1.2/COVERAGE.md). See
[Technical Specification (Japanese)](docs/v0.1.2/SPECS_JA.md) for the full design.

## Roadmap

See [`docs/v0.1.2/OPERATIONS_JA.md` §9](docs/v0.1.2/OPERATIONS_JA.md) for current priorities. Headline items:

- Additional processors: `provenance-graph`, `image-pdq`, `video-vpdq`, `cert-google/sony/leica`
- TypeScript client SDK
- Range Request streaming for large content fetch
- Mainnet contract deployment with multisig admin

## Documentation

| Document | Description |
|---|---|
| [Technical Spec (JA)](docs/v0.1.2/SPECS_JA.md) | Full protocol specification (v0.1.2, Japanese) |
| [Operations (JA)](docs/v0.1.2/OPERATIONS_JA.md) | Deploy + lifecycle + troubleshooting guide |
| [Coverage](docs/v0.1.2/COVERAGE.md) | Spec-to-implementation mapping |
| [deploy/aws/README.md](deploy/aws/README.md) | AWS Nitro Enclave deployment runbook |
| [docs/README.md](docs/README.md) | Documentation structure and version history |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

See [SECURITY.md](SECURITY.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).