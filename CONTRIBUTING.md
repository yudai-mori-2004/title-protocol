# Contributing to Title Protocol

Thank you for your interest in contributing to Title Protocol.

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.82+ | TEE server, Gateway, Processors |
| Node.js | 20+ | (Future: client SDK, tooling) |

## Getting Started

```bash
git clone https://github.com/yudai-mori-2004/title-protocol.git
cd title-protocol

# Build and test (once implementation exists)
cargo check --workspace
cargo test --workspace
```

## Project Structure

```
docs/                 -- Versioned documentation (SPECS -> COVERAGE -> tasks)
  v0.1.2/             -- Current version (full rewrite)
    SPECS_JA.md        -- Technical specification (Japanese)
    COVERAGE.md        -- Spec-to-implementation mapping
    tasks/             -- AI development task definitions
  v0.1.0/              -- Archived spec + tasks
  v0.1.1/              -- Archived spec + tasks
legacy/v0.1.0/         -- Archived v0.1.0 implementation (reference only)
```

Implementation code will be added as v0.1.2 tasks are completed. The crate structure is defined by the specification but not yet created.

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
- Do not modify legacy code (`legacy/v0.1.0/`)

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
