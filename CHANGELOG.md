# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased] — v0.1.2

Full protocol rewrite. See [Technical Spec](docs/v0.1.2/SPECS_JA.md).

### Changed
- **Trust model**: Collection-based -> Attestation Document-based
- **Module system**: WASM sandboxed modules -> Rust-native Processors compiled into TEE binary
- **Communication**: Client -> Temp Storage -> TEE -> Solana replaced with Client -> Gateway -> TEE -> External Storage (direct fetch)
- **Encryption**: Mandatory E2EE -> Optional (x25519 / p256 / ml-kem-768)
- **Blockchain**: Core component (GlobalConfig PDA) -> Extension layer (Solana Extension)
- **Architecture**: 7 crates + proxy + WASM host -> Gateway + TEE (2 components)

### Added
- Fragmented input support (CMAF streaming segments)
- Sidecar input support (detached C2PA manifest)
- HTTP Range Request for large file processing without full memory load
- ResourcePool + Ticket memory management
- ML-KEM-768 post-quantum encryption suite (FIPS 203)
- Direction-separated key derivation (request_key / response_key)
- ZK proof (SP1 zkVM) for on-chain TEE signing key whitelist
- Developer-managed collections for Solana Extension

### Removed
- WASM execution engine (wasmtime)
- TEE HTTP proxy
- Temporary storage layer
- GlobalConfig PDA (replaced by whitelist PDA)
- `image-phash` processor (deprecated, replaced by `image-pdq`)
- `cert-rootlens` processor (removed from initial processor list)

## [0.1.0] — 2026-03-02

Initial open-source release.

### Added
- **Core protocol**: C2PA verification, provenance graph construction, WASM extension execution
- **TEE server**: /verify, /sign, /create-tree endpoints with `TeeRuntime` trait abstraction
- **Gateway**: HTTP API server with `TempStorage` trait abstraction
- **Proxy**: HTTP proxy for TEE network isolation (TCP with socat-to-vsock bridge)
- **Cryptography**: X25519 ECDH, HKDF-SHA256, AES-256-GCM, Ed25519, TEE attestation verification
- **WASM modules**: phash-v1, hardware-google, c2pa-training-v1, c2pa-license-v1
- **TypeScript SDK**: Client library with E2EE encryption
- **Indexer**: cNFT event indexer (webhook + poller + DAS API)
- **Solana program**: GlobalConfig PDA management with on-chain ResourceLimits (Anchor)
- **CLI**: Rust CLI for devnet initialization, node registration/removal, tree creation
- **Vendor implementations**: AWS Nitro Enclaves (`vendor-aws`), local development (`vendor-local`)
- **Deployment**: Terraform + setup scripts for multi-node AWS Nitro, local docker-compose
- **CI/CD**: GitHub Actions (check, test, audit, WASM build, TypeScript build, npm publish)
- **QUICKSTART**: Step-by-step guide for local node and devnet deployment
