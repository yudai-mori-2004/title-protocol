<!-- AI coding assistant instructions. Also useful as a human developer reference. -->

# CLAUDE.md

## Project Overview

Title Protocol: C2PA署名付きコンテンツから属性を抽出し、Attestation Documentで封印する属性抽出レイヤー。
TEE（Trusted Execution Environment）内で処理することで、コンテンツの生データを開示せずに検証結果を第三者に証明できる。

- Documentation: `docs/README.md` (versioned: SPECS -> COVERAGE -> tasks)
- Current version: `docs/v0.1.2/` (full rewrite from v0.1.0)
  - Technical spec: `docs/v0.1.2/SPECS_JA.md` (Japanese)
  - Coverage: `docs/v0.1.2/COVERAGE.md`
  - Tasks: `docs/v0.1.2/tasks/NN-name/`
- Legacy code: `legacy/v0.1.0/` (v0.1.0 implementation, reference only)

## Session Protocol

### Session Start

1. Read this file (CLAUDE.md)
2. Read the task README specified by the user: `docs/v0.1.2/tasks/NN-name/README.md`
3. Read `docs/v0.1.2/COVERAGE.md` to understand current implementation status
4. If the task requires spec understanding, read `docs/v0.1.2/SPECS_JA.md` (1177 lines, full read recommended)

### Context Recovery (compact / session break)

Session summaries should include:
- Which task was being worked on
- Which files were created/modified
- What remains to be done
- Any design decisions made during the session

The next session can recover by reading CLAUDE.md + task README + COVERAGE.md.

### Session End

1. Update `docs/v0.1.2/COVERAGE.md` with implementation progress
2. Verify build passes (once build infrastructure exists)
3. Do NOT commit or push without explicit user permission

### 1 task = 1 session

To prevent context overflow. Each task is scoped to fit within a single session.

## Architecture (v0.1.2)

```
Client ──→ Gateway ──→ TEE ──→ External Storage (user-managed)
```

2 components only. No Temp Storage, no Proxy.

| Component | Runtime | Role | Spec |
|---|---|---|---|
| Gateway | Normal server (EC2 etc.) | Client auth, TEE info relay, request proxy | SPECS_JA §5.3 |
| TEE | Trusted Execution Environment (AWS Nitro Enclaves etc.) | Content fetch, C2PA verify, attribute extraction, Attestation | SPECS_JA §5.2 |

### Trust Model

Attestation Document based (NOT collection based). The TEE hardware generates a certificate proving:
- What code was running (PCR measurement)
- What the output was (user_data = SHA-256 of results)

### Processors (compiled into TEE binary, NOT WASM)

| Processor | Role | Spec |
|---|---|---|
| `c2pa-verify` | C2PA signature chain verification (mandatory for all requests) | §3.2 |
| `provenance-graph` | Ingredient DAG extraction | §3.2 |
| `image-pdq` | PDQ 256-bit perceptual hash | §3.2 |
| `video-vpdq` | Per-frame vPDQ hash | §3.2 |
| `cert-google` | Google C2PA root CA chain verification | §3.2 |
| `cert-sony` | Sony C2PA root CA chain verification | §3.2 |
| `cert-leica` | Leica C2PA root CA chain verification | §3.2 |

### Input Types

| Type | Description | Spec |
|---|---|---|
| `single` | Single file (JPEG, PNG, MP4). Large files via HTTP Range Request | §1.3, §4.3 |
| `fragmented` | CMAF segments (init.mp4 + seg-*.m4s). Process one segment at a time | §1.3, §4.3 |
| `sidecar` | Separate manifest (.c2pa) + content file | §1.3, §4.3 |

### Encryption (Optional)

| Suite | suite_id | Key Exchange | Spec |
|---|---|---|---|
| `x25519` | `0x01` | X25519 ECDH | §2.4 |
| `p256` | `0x02` | ECDH P-256 | §2.4 |
| `ml-kem-768` | `0x03` | ML-KEM-768 (FIPS 203) | §2.4 |

Direction-separated key derivation: request_key and response_key derived independently from shared secret via HKDF-SHA256.

### Extension Layer

| Extension | Role | Spec |
|---|---|---|
| Solana Extension | cNFT minting with ZK-proven TEE signing key whitelist | §6 |

Solana Extension uses SP1 zkVM to verify Attestation Documents on-chain. Developer-managed collections (not protocol-managed).

## Build

```bash
# Workspace build (all crates)
cargo check --workspace
cargo test --workspace

# With vendor-aws feature (includes AWS Nitro skeleton)
cargo test --workspace --features title-tee/vendor-aws

# SP1 guest (out-of-workspace; uses pinned v5.2.4 toolchain)
cd sp1-guests/attestation-aws-nitro/host && cargo run --release --bin vkey
```

## Coding Conventions

- All Rust public functions have doc comments (Japanese) with spec section references (e.g., `/// 仕様書 §3.2 c2pa-verify`)
- Error types defined with `thiserror`, one Error enum per crate
- Struct field names match spec JSON structures (snake_case)
- Processors are native Rust modules compiled into TEE binary (NOT WASM)
- Tests in `#[cfg(test)] mod tests` within each crate
- Completed versions (`docs/v0.1.0/`, `docs/v0.1.1/`) are read-only archives

## Key Design Decisions

- **No WASM** — Processors are Rust-native, compiled directly into TEE binary (§3.3)
- **No Proxy** — TEE communicates directly via Gateway (§5.1)
- **No Temp Storage** — TEE fetches content directly from external URLs (§5.2)
- **Attestation Document as trust anchor** — replaces collection-based trust model (§1.2)
- **E2EE is optional** — `encryption` field omitted = plaintext (§1.4)
- **Blockchain is Extension** — Solana moved to Extension layer, not core (§6)
- **TEE is stateless** — no state between requests, keys in memory only (§0.5)
- **Vendor separation via feature flags** — `vendor-aws` for AWS Nitro-specific code
- **TEE runtime is trait-abstracted** — `trait TeeRuntime` for vendor implementations

## Legacy Code Reference

**設計や実装で迷ったら、まず `legacy/v0.1.0/` を読め。** 同じプロトコルの前バージョンであり、型設計・trait構成・crate分割の判断材料が揃っている。ゼロから考える前にまず既存の設計を確認すること。

v0.1.0 implementation is archived at `legacy/v0.1.0/`. Useful for:
- Crate structure and workspace layout (`legacy/v0.1.0/Cargo.toml`, `legacy/v0.1.0/crates/`)
- Type definitions and data models (`legacy/v0.1.0/crates/types/`)
- PDQ/vPDQ hash algorithm implementations (`legacy/v0.1.0/wasm/image-pdq/`, `legacy/v0.1.0/wasm/video-vpdq/`)
- Certificate chain verification logic (`legacy/v0.1.0/wasm/cert-*/`)
- Crypto primitives — AES-GCM, HKDF, X25519 (`legacy/v0.1.0/crates/crypto/`)
- c2pa-rs integration patterns (`legacy/v0.1.0/crates/core/`)
- TEE trait abstraction (`legacy/v0.1.0/crates/tee/`)
- Axum server patterns (`legacy/v0.1.0/crates/gateway/`, `legacy/v0.1.0/crates/tee/`)

Note: v0.1.0 WASM modules are `#![no_std]` + dlmalloc. v0.1.2 processors are standard Rust — port the algorithms, not the scaffolding.

## Key Dependencies (v0.1.2)

| Crate | Version | Role |
|---|---|---|
| `c2pa` | 0.84+ | C2PA verification (was 0.78 in v0.1.0). Builder API: `Reader::default().with_stream()` |
| `http-range-client` | latest | Read+Seek over HTTP Range Requests for large file processing |
| `sp1-sdk` / `sp1-zkvm` | =5.2.4 (pinned) | ZK proof generation for Attestation Document verification — pinned to v5 because `sp1-solana` 0.1.0 is hard-wired to v5 wire format; v6 proofs are rejected on-chain. See `docs/v0.1.2/tasks/15-docker-deployment/PCR0_REPRODUCIBILITY_INVESTIGATION.md`. |
| `sp1-solana` | 0.1.0 | On-chain Groth16 proof verification (~280K CU) |
| `ml-kem` | latest | ML-KEM-768 (FIPS 203) post-quantum key exchange |

## Constraints

- Do NOT commit or push without explicit user permission
- Do NOT push before device/integration testing
- Use general terms for off-chain storage ("off-chain storage", "the URL target"), never guess specific service names
- Design first, no "quick and dirty" implementations
- Work autonomously — anticipate next steps without waiting for each instruction
