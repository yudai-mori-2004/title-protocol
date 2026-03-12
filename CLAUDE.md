<!-- AI coding assistant instructions. Also useful as a human developer reference. -->

# CLAUDE.md

## Project Overview

Title Protocol: Records digital content attribution on Solana blockchain.
Combines C2PA (provenance) x TEE (Trusted Execution Environment) x cNFT (compressed NFT) to trustlessly resolve content ownership.

- Documentation: `docs/README.md` (versioned: SPECS -> COVERAGE -> tasks)
- Current version: `docs/v0.1.0/` (2026-02-21, initial implementation)
  - Technical spec: `docs/v0.1.0/SPECS_JA.md` (ver.9, Japanese)
  - Coverage: `docs/v0.1.0/COVERAGE.md`
  - Tasks: `docs/v0.1.0/tasks/NN-name/`
- Environment variables: `.env.example`

## Build

```bash
# Rust workspace (7 crates)
cargo check --workspace
cargo test --workspace

# WASM modules (4 modules, excluded from workspace — build individually)
cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release
cd wasm/hardware-google && cargo build --target wasm32-unknown-unknown --release
cd wasm/c2pa-training-v1 && cargo build --target wasm32-unknown-unknown --release
cd wasm/c2pa-license-v1 && cargo build --target wasm32-unknown-unknown --release

# TypeScript SDK
cd sdk/ts && npm run build

# TypeScript Indexer
cd indexer && npm run build

# Anchor program (requires cargo-build-sbf)
cd programs/title-config && rm -f Cargo.lock && cargo generate-lockfile && cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

## Coding Conventions

- All Rust public functions have doc comments (Japanese) with spec section references (e.g., `/// 仕様書 §5.1 Step 4`)
- Error types defined with `thiserror`, one Error enum per crate
- Struct field names match spec JSON structures (snake_case)
- WASM modules: `#![no_std]` + `dlmalloc` global allocator + `core::arch::wasm32::unreachable()` panic handler
- Tests in `#[cfg(test)] mod tests` within each crate
- Completed versions (`docs/v0.1.0/` etc.) are read-only archives

## Architecture

```
Client (SDK) → Gateway → Temporary Storage → TEE → Solana
                                              ↓
                                         Off-chain Storage (Arweave)
```

### Rust Crates (workspace members)

| Crate | Role | Spec |
|-------|------|------|
| `crates/types` | Shared type definitions | §5 |
| `crates/crypto` | Cryptographic primitives (ECDH, AES-GCM, Ed25519, SHA-256) | §1.1, §6.4 |
| `crates/core` | C2PA verification + provenance graph construction | §2.1, §2.2 |
| `crates/wasm-host` | WASM execution engine (wasmtime) | §7.1 |
| `crates/tee` | TEE server (axum) | §6.4, §1.1 |
| `crates/gateway` | Gateway HTTP server (axum) | §6.2 |
| `crates/proxy` | TEE HTTP proxy | §6.4 |

### WASM Modules (outside workspace, build individually)

| Module | Output | Spec |
|--------|--------|------|
| `wasm/phash-v1` | Perceptual hash | §7.4 |
| `wasm/hardware-google` | Hardware capture proof | §7.4 |
| `wasm/c2pa-training-v1` | AI training consent flag | §7.4 |
| `wasm/c2pa-license-v1` | License information | §7.4 |

### TypeScript

| Package | Role | Spec |
|---------|------|------|
| `sdk/ts` | Client SDK (register, resolve, discover) | §6.7 |
| `indexer` | cNFT indexer (webhook + poller) | §6.6 |

### Solana Program

| Program | Role | Spec |
|---------|------|------|
| `programs/title-config` | Global Config PDA management (Anchor) | §8 |

## Key Design Decisions

- **No Extism** — use wasmtime directly (§7.1)
- **c2pa crate v0.75**
- **Vendor separation via feature flags**: `vendor-aws` feature gates vendor-specific code. Proxy uses `#[cfg(target_os = "linux")]` for conditional compilation
- **TEE runtime is trait-abstracted**: `trait TeeRuntime` → `MockRuntime` (local) / vendor implementations (behind feature flags)
- **TEE is stateless**: No state between requests. Keys exist only in memory, lost on restart
- **Proxy protocol**: length-prefixed format
  - TEE→Proxy: `[4B: method_len][method][4B: url_len][url][4B: body_len][body]`
  - Proxy→TEE: `[4B: status_code][4B: body_len][body]`

## Task Workflow

Each task is defined in `docs/vN/tasks/NN-name/README.md`. At session start, read the specified task's README and follow its requirements, files to read, and completion criteria. Notes and learnings go in `.md` files in the same task directory.

**1 task = 1 session** to prevent context overflow.

After completing work:
1. Update `docs/vN/COVERAGE.md`
2. Verify `cargo check --workspace && cargo test --workspace` passes
