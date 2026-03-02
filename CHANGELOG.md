# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - 2026-03-02

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
