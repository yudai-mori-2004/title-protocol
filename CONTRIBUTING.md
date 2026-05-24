# Contributing to Title Protocol

Thank you for your interest in contributing to Title Protocol.

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | pinned in `rust-toolchain.toml` (1.93.1) | TEE, Gateway, Proxy, Crypto |
| Docker + Compose | recent | Local mock stack |
| Anchor CLI | 0.30.x | Solana program (`programs/title-whitelist`) |
| Solana CLI | 2.x | Devnet / Localnet interaction |

## Getting Started

```bash
git clone https://github.com/yudai-mori-2004/title-protocol.git
cd title-protocol

# Local mock stack (TEE in mock runtime + Gateway):
docker compose up --build -d
./docker/smoke-test.sh

# Or build + test directly:
cargo test --workspace
cargo clippy --workspace --all-targets --features title-tee/runtime-mock
```

### Solana program build

The Anchor program at `programs/title-whitelist/` lives outside the Cargo
workspace (Solana toolchain conflict). Build it with:

```bash
cd programs/title-whitelist && anchor build --no-idl
```

See [`docs/v0.1.2/OPERATIONS_JA.md`](docs/v0.1.2/OPERATIONS_JA.md) §2.2 for the
full deployment flow.

## Project Structure

```
crates/                 -- Rust workspace: core, crypto, attestation, tee,
                           gateway, proxy, solana, attestation-aws-nitro
programs/title-whitelist/  -- Anchor program (devnet 43y8E...; separate workspace)
sp1-guests/             -- SP1 zkVM guest + host (separate workspaces)
docs/                   -- Versioned documentation (SPECS -> COVERAGE -> tasks)
  v0.1.2/               -- Current version
    SPECS_JA.md         -- Technical specification (Japanese, source of truth)
    OPERATIONS_JA.md    -- Deploy + lifecycle + troubleshooting
    COVERAGE.md         -- Spec-to-implementation mapping
    tasks/              -- Per-session task definitions
deploy/aws/             -- Terraform + Dockerfiles for AWS Nitro deployment
docker/                 -- Mock-runtime Dockerfile + smoke test
```

The earlier `v0.1.0` source tree is **not** kept in-tree; consult the
`v0.1.0` git tag (or `docs/v0.1.0/`) when historical context is needed.

## Coding Standards

### Rust

- **Doc comments in Japanese** with specification section references (e.g., `/// 仕様書 §3.2 c2pa-verify`)
- **Error types** use `thiserror` with a dedicated Error enum per crate
- **JSON field names** match specification structs (snake_case)
- **Processors** are native Rust modules compiled into the TEE binary
- **Tests** are written as `#[cfg(test)] mod tests` within each crate

### General

- Keep changes focused and minimal
- Do not modify archived version docs (`docs/v0.1.0/`, `docs/v0.1.1/`) unless fixing errors

## AI-Driven Development

This project uses an AI-driven development workflow:

- **`CLAUDE.md`** at the repository root provides instructions for AI coding assistants
- **`docs/v0.1.2/`** contains the current spec, coverage matrix, and task definitions
- Each task is defined in `docs/v0.1.2/tasks/NN-name/README.md`
- **1 task = 1 session** to prevent context overflow

## Pull Request Process

1. One task = one PR. Keep pull requests focused on a single logical change.
2. Create a feature branch from `main`.
3. Ensure all tests pass before submitting.
4. Write a clear PR description explaining the what and why of your changes.
5. Reference related task documents where applicable.

## Reporting Issues

- Use [GitHub Issues](https://github.com/yudai-mori-2004/title-protocol/issues) for bug reports and feature requests
- For security vulnerabilities, see [SECURITY.md](SECURITY.md)

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
