# Contributing to Title Protocol

Thank you for your interest in contributing to Title Protocol. This document provides guidelines and information for contributors.

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.82+ | Core logic, TEE, Gateway, Proxy, WASM modules |
| Node.js | 24+ | TypeScript SDK, Indexer, scripts |
| Solana CLI | 1.18+ | Blockchain interaction, key management |
| Anchor CLI | 0.30+ | Solana program builds (optional) |

## Getting Started

```bash
# Clone the repository
git clone https://github.com/yudai-mori-2004/title-protocol.git
cd title-protocol

# Build and test the Rust workspace
cargo check --workspace
cargo test --workspace

# Build WASM modules (excluded from workspace, built individually)
for dir in wasm/*/; do
  cargo build --manifest-path "${dir}Cargo.toml" --target wasm32-unknown-unknown --release
done

# Build TypeScript packages
cd sdk/ts && npm ci && npm run build && cd ../..
cd indexer && npm ci && npm run build && cd ..
```

## Running Tests

```bash
# Rust unit tests (all crates)
cargo test --workspace

# TypeScript SDK tests
cd sdk/ts && npm run build && npm test

# TypeScript Indexer tests
cd indexer && npm run build && npm test
```

See `.env.example` for all configuration options.

## Project Structure

```
crates/           — Rust workspace (types, crypto, core, wasm-host, tee, gateway, proxy)
wasm/             — WASM modules (phash-v1, hardware-google, c2pa-training-v1, c2pa-license-v1)
programs/         — Solana Anchor program (title-config)
sdk/ts/           — TypeScript client SDK
indexer/          — TypeScript cNFT indexer
deploy/           — Vendor-specific deployment
docker/           — Container images
docs/             — Versioned development documentation
```

## Vendor Feature Flags

The codebase separates **protocol core** (vendor-neutral) from **vendor implementations** using Cargo feature flags:

```bash
# Build protocol core only
cargo check --workspace --no-default-features

# Build with AWS vendor implementation (default)
cargo check --workspace
```

The `vendor-aws` feature includes: S3-based `TempStorage` implementation, AWS Nitro `TeeRuntime` implementation, and vsock transport. The `TeeRuntime` trait (`crates/tee/src/runtime/`) and `TempStorage` trait (`crates/gateway/src/storage/`) define the vendor-neutral interfaces — alternative implementations (other cloud TEEs, MinIO, etc.) can be added by implementing these traits.

## Coding Standards

### Rust

- **Doc comments in Japanese** with specification section references (e.g., `/// 仕様書 §5.1 Step 4`)
- **Error types** use `thiserror` with a dedicated Error enum per crate
- **JSON field names** match specification structs (snake_case)
- **WASM modules** use `#![no_std]` + `dlmalloc` global allocator + custom panic handler
- **Tests** are written as `#[cfg(test)] mod tests` within each crate

### TypeScript

- Strict TypeScript (`strict: true`)
- CommonJS module format
- Tests use Node.js built-in test runner (`node --test`)

### General

- Keep changes focused and minimal
- Do not modify completed version docs (`docs/v1/` etc.) unless fixing errors

## Building the Solana Program

The Anchor program (`programs/title-config`) has specific build requirements due to Solana toolchain constraints.

```bash
# Install Anchor CLI (LTO disabled to avoid LLVM version mismatch on macOS)
CARGO_PROFILE_RELEASE_LTO=off cargo install anchor-cli --version 0.30.1

# Build with cargo-build-sbf (NOT anchor build)
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

**Key constraints:**
- Use `cargo-build-sbf` directly instead of `anchor build` (to specify `--tools-version`)
- Platform Tools **v1.52** is required because some dependencies use Rust Edition 2024
- The program has its own `Cargo.lock` separate from the workspace — regenerate it before building
- `Anchor.toml` must be at the project root (not inside `programs/`)

## Pull Request Process

1. **One task = one PR.** Keep pull requests focused on a single logical change.
2. Create a feature branch from `main` with a descriptive name (e.g., `feat/add-xyz`, `fix/issue-123`).
3. Ensure all tests pass before submitting:
   ```bash
   cargo check --workspace && cargo test --workspace
   cd sdk/ts && npm run build
   cd indexer && npm run build
   ```
4. Write a clear PR description explaining the what and why of your changes.
5. Reference related issues or task documents where applicable.

## AI-Driven Development

This project uses an AI-driven development workflow:

- **`CLAUDE.md`** at the repository root provides instructions for AI coding assistants
- **`docs/`** contains versioned documentation: SPECS (what to build) -> COVERAGE (what's built) -> tasks (how it was built)
- Each task is defined in `docs/vN/tasks/NN-name/README.md`

## Reporting Issues

- Use [GitHub Issues](https://github.com/yudai-mori-2004/title-protocol/issues) for bug reports and feature requests
- For security vulnerabilities, see [SECURITY.md](SECURITY.md)

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
