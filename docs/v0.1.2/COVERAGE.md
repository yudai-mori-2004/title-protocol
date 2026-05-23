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
| §1.3 | Processor execution framework | [~] | crates/core/src/processor.rs (Processor trait + ProcessorRegistry) | 02 |
| §1.3 | c2pa-verify (mandatory, signature_hash) | [x] | crates/core/src/c2pa_verify.rs (C2paVerifyProcessor, compute_signature_hash utility) | 03 |
| §1.3 | Input type: single file | [~] | crates/core/src/request.rs (InputData::Single type defined) | 02 |
| §1.3 | Input type: fragmented (CMAF) | [~] | crates/core/src/request.rs (InputData::Fragmented type defined) | 02 |
| §1.3 | Input type: sidecar | [~] | crates/core/src/request.rs (InputData::Sidecar type defined) | 02 |
| §1.4 | Encryption (optional, x25519/p256/ml-kem-768) | [~] | crates/core/src/request.rs (EncryptionSuite enum defined, logic not implemented) | 02 |
| §1.5 | Verification model (JCS + hash comparison) | [ ] | | |
| §1.7 | Gateway role definition | [ ] | | |

### 2. Communication Model (§2)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §2.1 | Request flow (Client → Gateway → TEE → External Storage) | [ ] | | |
| §2.2 | Request format (single/fragmented/sidecar JSON) | [x] | crates/core/src/request.rs (ProcessRequest + InputData, serde tests match spec JSON) | 02 |
| §2.3 | Response format (signature_hash + results + attestation) | [x] | crates/core/src/response.rs (ProcessResponse + VerifiableResponse, serde tests match spec JSON) | 02 |
| §2.4 | Key bundle (per-suite key pair generation at startup) | [ ] | | |
| §2.4 | Encryption flow (12-step client-TEE exchange) | [ ] | | |
| §2.4 | Direction-separated key derivation (HKDF) | [ ] | | |
| §2.4 | Wire format (request: suite_id + encap_key + nonce + ciphertext) | [ ] | | |
| §2.4 | Wire format (response: nonce + ciphertext) | [ ] | | |
| §2.4 | Encrypted payload internal structure (metadata_len + JSON + raw binary) | [ ] | | |
| §2.5 | Gateway API: GET /keys | [~] | crates/gateway/src/lib.rs (KeysResponse type defined, HTTP handler not implemented) | 02 |
| §2.5 | Gateway API: GET /processors | [~] | crates/gateway/src/lib.rs (ProcessorsResponse type defined, HTTP handler not implemented) | 02 |
| §2.5 | Gateway API: POST /process | [~] | crates/gateway/src/lib.rs (types defined via title-core re-export, HTTP handler not implemented) | 02 |
| §2.5 | Gateway API: GET /health | [~] | crates/gateway/src/lib.rs (HealthResponse type defined, HTTP handler not implemented) | 02 |
| §2.5 | Gateway API: GET /solana-keys | [~] | crates/gateway/src/lib.rs (SolanaKeysResponse type defined, HTTP handler not implemented) | 02 |
| §2.5 | Gateway API: POST /extension/solana | [~] | crates/gateway/src/lib.rs (SolanaExtensionRequest/Response types defined, HTTP handler not implemented) | 02 |

### 3. Processors (§3)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §3.1 | Processor trait/interface definition | [x] | crates/core/src/processor.rs (Processor trait + ProcessorRegistry + ProcessorError, 7 tests) | 02 |
| §3.2 | c2pa-verify processor | [x] | crates/core/src/c2pa_verify.rs (C2paVerifyProcessor + compute_signature_hash, JUMBF parser in jumbf.rs, 14 tests) | 03 |
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
| §5.2 | TEE startup sequence (key generation → notify Gateway) | [~] | crates/tee/src/lib.rs (TeeRuntime trait defined, startup logic not implemented) | 02 |
| §5.2 | TEE request processing flow | [~] | crates/tee/src/lib.rs (TeeRuntime trait defined, flow not implemented) | 02 |
| §5.2 | Content fetch: single (HTTP Range Request + ETag) | [~] | sandbox/01-c2pa-range-request/ (sandbox verified: sig verify, tamper detect, Range Request parity — all PASS) | 01 |
| §5.2 | Content fetch: fragmented | [~] | sandbox/02-c2pa-fragment/ (sandbox verified: sign/verify roundtrip, tamper detect, partial verify — all PASS) | 01 |
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
| §6.2 | Solana Extension: ZK proof generation (SP1 zkVM) | [~] | sandbox/03-sp1-attestation/ (sandbox verified: cert chain verify, core proof gen/verify, tamper detect 3/3 — all PASS. 96M cycles, 169B public values, Groth16 ~479B fits Solana 1,232B. Attestation verify internalized, zero git deps) | 01 |
| §6.2 | Solana Extension: Whitelist PDA + ZK proof verification | [ ] | | |
| §6.2 | Solana Extension: Developer collection setup + delegate | [ ] | | |
| §6.2 | Solana Extension: cNFT mint (partial signing) | [ ] | | |
| §6.2 | Solana Extension: Signing key expiry (90-day rotation) | [ ] | | |
| §6.2 | Solana Extension: Whitelist key deletion (emergency) | [ ] | | |
