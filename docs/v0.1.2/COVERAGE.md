# COVERAGE — v0.1.2

> v0.1.2 is a full rewrite. No carryover from v0.1.0/v0.1.1.

Spec: `docs/v0.1.2/SPECS_JA.md`

## Status Legend

- [ ] Not started
- [~] In progress
- [x] Complete

## Coverage Matrix

### 1. Protocol Model (§1)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §1.2 | Attestation Document integration (user_data embedding) | [ ] | | |
| §1.3 | Processor execution framework | [ ] | | |
| §1.3 | c2pa-verify (mandatory, signature_hash) | [ ] | | |
| §1.3 | Input type: single file | [ ] | | |
| §1.3 | Input type: fragmented (CMAF) | [ ] | | |
| §1.3 | Input type: sidecar | [ ] | | |
| §1.4 | Encryption (optional, x25519/p256/ml-kem-768) | [ ] | | |
| §1.5 | Verification model (JCS + hash comparison) | [ ] | | |
| §1.7 | Gateway role definition | [ ] | | |

### 2. Communication Model (§2)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §2.1 | Request flow (Client → Gateway → TEE → External Storage) | [ ] | | |
| §2.2 | Request format (single/fragmented/sidecar JSON) | [ ] | | |
| §2.3 | Response format (signature_hash + results + attestation) | [ ] | | |
| §2.4 | Key bundle (per-suite key pair generation at startup) | [ ] | | |
| §2.4 | Encryption flow (12-step client-TEE exchange) | [ ] | | |
| §2.4 | Direction-separated key derivation (HKDF) | [ ] | | |
| §2.4 | Wire format (request: suite_id + encap_key + nonce + ciphertext) | [ ] | | |
| §2.4 | Wire format (response: nonce + ciphertext) | [ ] | | |
| §2.4 | Encrypted payload internal structure (metadata_len + JSON + raw binary) | [ ] | | |
| §2.5 | Gateway API: GET /keys | [ ] | | |
| §2.5 | Gateway API: GET /processors | [ ] | | |
| §2.5 | Gateway API: POST /process | [ ] | | |
| §2.5 | Gateway API: GET /health | [ ] | | |
| §2.5 | Gateway API: GET /solana-keys | [ ] | | |
| §2.5 | Gateway API: POST /extension/solana | [ ] | | |

### 3. Processors (§3)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §3.1 | Processor trait/interface definition | [ ] | | |
| §3.2 | c2pa-verify processor | [ ] | | |
| §3.2 | provenance-graph processor | [ ] | | |
| §3.2 | image-pdq processor | [ ] | | |
| §3.2 | video-vpdq processor | [ ] | | |
| §3.2 | cert-google processor | [ ] | | |
| §3.2 | cert-sony processor | [ ] | | |
| §3.2 | cert-leica processor | [ ] | | |

### 4. Memory Management (§4)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §4.1 | ResourcePool (admission_limit / total_limit) | [ ] | | |
| §4.2 | Ticket (incremental reservation / release) | [ ] | | |
| §4.3 | Memory pattern: single file (Range Request) | [ ] | | |
| §4.3 | Memory pattern: fragmented | [ ] | | |
| §4.3 | Memory pattern: sidecar | [ ] | | |
| §4.4 | Data size limits enforcement | [ ] | | |
| §4.4 | Chunk timeout (60s) | [ ] | | |
| §4.4 | Global timeout (max 30min, size-adaptive) | [ ] | | |
| §4.4 | Decode memory protection (header-based estimation) | [ ] | | |

### 5. System Implementation (§5)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §5.2 | TEE startup sequence (key generation → notify Gateway) | [ ] | | |
| §5.2 | TEE request processing flow | [ ] | | |
| §5.2 | Content fetch: single (HTTP Range Request + ETag) | [ ] | | |
| §5.2 | Content fetch: fragmented | [ ] | | |
| §5.2 | Content fetch: sidecar | [ ] | | |
| §5.3 | Gateway: client auth + rate limiting | [ ] | | |
| §5.3 | Gateway: TEE info relay | [ ] | | |
| §5.3 | Gateway: request proxy | [ ] | | |
| §5.3 | Gateway: TEE restart detection + key refresh | [ ] | | |
| §5.4 | Reproducible build (Dockerfile, Cargo.lock, toolchain pinning) | [ ] | | |

### 6. Extension (§6)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §6.1 | Extension framework (core result → extension request) | [ ] | | |
| §6.2 | Solana Extension: Ed25519 signing key generation | [ ] | | |
| §6.2 | Solana Extension: Attestation Document for signing key | [ ] | | |
| §6.2 | Solana Extension: ZK proof generation (SP1 zkVM) | [ ] | | |
| §6.2 | Solana Extension: Whitelist PDA + ZK proof verification | [ ] | | |
| §6.2 | Solana Extension: Developer collection setup + delegate | [ ] | | |
| §6.2 | Solana Extension: cNFT mint (partial signing) | [ ] | | |
| §6.2 | Solana Extension: Signing key expiry (90-day rotation) | [ ] | | |
| §6.2 | Solana Extension: Whitelist key deletion (emergency) | [ ] | | |
