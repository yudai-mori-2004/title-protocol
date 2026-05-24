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
| §1.2 | Attestation Document integration (user_data embedding) | [x] | crates/tee/src/orchestrator.rs (JCS hash → user_data binding, 2 tests) | 04 |
| §1.2 | Vendor root CA fingerprint pinning | [x] | crates/attestation-aws-nitro/src/constants.rs (AWS_NITRO_ROOT_CA_SHA256) + doc.rs::authenticate (real Nitro fixture passes) | post-15 |
| §1.3 | Processor execution framework | [x] | crates/core/src/processor.rs (Processor trait + ProcessorRegistry) + crates/tee/src/orchestrator.rs (process_request pipeline, 9 tests) | 02, 04 |
| §1.3 | c2pa-verify (mandatory, signature_hash) | [x] | crates/core/src/c2pa_verify.rs (C2paVerifyProcessor, compute_signature_hash utility) | 03 |
| §1.3 | Input type: single file | [~] | crates/core/src/request.rs (InputData::Single type defined) | 02 |
| §1.3 | Input type: fragmented (CMAF) | [~] | crates/core/src/request.rs (InputData::Fragmented type defined) | 02 |
| §1.3 | Input type: sidecar | [~] | crates/core/src/request.rs (InputData::Sidecar type defined) | 02 |
| §1.4 | Encryption (optional, x25519/p256/ml-kem-768) | [x] | crates/core/src/request.rs (EncryptionSuite enum) + crates/crypto/ (KEM×3, HKDF, AES-256-GCM, wire format, sealed channel, 27 tests) + crates/tee/src/orchestrator.rs (sealed channel wired into request pipeline, 3 e2e tests) | 02, 11, post-15 |
| §1.5 | Verification model (JCS + hash comparison) | [x] | crates/tee/src/orchestrator.rs (compute_jcs_hash, serde_json_canonicalizer, 3 tests) | 04 |
| §1.7 | Gateway role definition | [x] | crates/gateway/src/ (Axum HTTP server: 6 endpoints, API key auth, rate limiting, TEE info caching, restart detection, 38 tests) | 10 |

### 2. Communication Model (§2)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §2.1 | Request flow (Client → Gateway → TEE → External Storage) | [~] | crates/tee/src/orchestrator.rs (TEE → External Storage fetch + processor pipeline implemented; Gateway relay not yet) | 04 |
| §2.2 | Request format (single/fragmented/sidecar JSON) | [x] | crates/core/src/request.rs (ProcessRequest + InputData, serde tests match spec JSON) | 02 |
| §2.3 | Response format (signature_hash + results + attestation) | [x] | crates/core/src/response.rs (ProcessResponse + VerifiableResponse, serde tests match spec JSON) | 02 |
| §2.4 | Key bundle (per-suite key pair generation at startup) | [x] | crates/crypto/src/key_bundle.rs (KeyBundle::generate: x25519 + p256 + ml-kem-768, public_keys Base64 export, 1 test) | 11 |
| §2.4 | Encryption flow (12-step client-TEE exchange) | [x] | crates/crypto/src/sealed_channel.rs (seal_for: KEM+HKDF+AES-256-GCM encrypt, open_request: TEE decrypt, ResponseChannel bidirectional, 6 tests) | 11 |
| §2.4 | Direction-separated key derivation (HKDF) | [x] | crates/crypto/src/hkdf.rs (HKDF-SHA256, salt=encap_key, info="title-request-key"/"title-response-key", 3 tests) | 11 |
| §2.4 | Wire format (request: suite_id + encap_key + nonce + ciphertext) | [x] | crates/crypto/src/wire.rs (build_request/parse_request, 6 tests) | 11 |
| §2.4 | Wire format (response: nonce + ciphertext) | [x] | crates/crypto/src/wire.rs (build_response/parse_response, 6 tests) | 11 |
| §2.4 | Encrypted payload internal structure (metadata_len + JSON + raw binary) | [x] | crates/crypto/src/payload.rs (build_payload/parse_payload, 3 tests) | 11 |
| §2.5 | Gateway API: GET /keys | [x] | crates/gateway/src/lib.rs (KeysResponse) + crates/gateway/src/endpoints.rs (handle_keys: cached TEE keys, 2 tests) | 02, 10 |
| §2.5 | Gateway API: GET /processors | [x] | crates/gateway/src/lib.rs (ProcessorsResponse) + crates/gateway/src/endpoints.rs (handle_processors, 1 test) | 02, 10 |
| §2.5 | Gateway API: POST /process | [x] | crates/gateway/src/lib.rs (types via title-core) + crates/gateway/src/endpoints.rs (handle_process: relay to TEE, 5 tests) | 02, 10 |
| §2.5 | Gateway API: GET /health | [x] | crates/gateway/src/lib.rs (HealthResponse) + crates/gateway/src/endpoints.rs (handle_health: cached TEE status, 2 tests) | 02, 10 |
| §2.5 | Gateway API: GET /solana-keys | [x] | crates/gateway/src/lib.rs (SolanaKeysResponse) + crates/gateway/src/endpoints.rs (handle_solana_keys: 404 when disabled, 2 tests) | 02, 10 |
| §2.5 | Gateway API: POST /extension/solana | [x] | crates/gateway/src/lib.rs (types) + crates/gateway/src/endpoints.rs (handle_solana_extension: relay to TEE, 2 tests) | 02, 10 |

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
| §4.1 | ResourcePool (admission_limit / total_limit) | [x] | crates/tee/src/resource_pool.rs (ResourcePool: 2-tier threshold, AtomicUsize counter, can_admit/try_admit/acquire, 5 tests) | 09 |
| §4.2 | Ticket (incremental reservation / release) | [x] | crates/tee/src/resource_pool.rs (Ticket: CAS-loop extend/shrink, RAII Drop auto-release, Cell<Instant> timeout tracking, 14 tests incl. 2 concurrent) | 09 |
| §4.3 | Memory pattern: single file (Range Request) | [x] | crates/tee/src/content_fetch.rs (fetch_single: ticket.extend on data arrival) + crates/tee/src/resource_pool.rs (Ticket API). Note: streaming Range Request with shrink is future optimization; current impl fetches full file. | 09 |
| §4.3 | Memory pattern: fragmented | [x] | crates/tee/src/content_fetch.rs (fetch_fragmented: ticket.extend per segment, validate_fragment_count/size) + resource_pool.rs. Note: accumulates all fragments; streaming shrink-per-fragment requires streaming C2PA reader (future). | 09 |
| §4.3 | Memory pattern: sidecar | [x] | crates/tee/src/content_fetch.rs (fetch_sidecar: ticket.extend for manifest + content separately) + resource_pool.rs | 09 |
| §4.4 | Data size limits enforcement | [x] | crates/tee/src/limits.rs (MAX_FRAGMENT_COUNT=100K, MAX_FRAGMENT_SIZE=100MB, validate_fragment_count/size, 4 tests) | 09 |
| §4.4 | Chunk timeout (60s) | [x] | crates/tee/src/resource_pool.rs (Ticket::extend checks Cell<Instant> last_activity against CHUNK_TIMEOUT) + crates/tee/src/limits.rs (CHUNK_TIMEOUT constant) | 09 |
| §4.4 | Global timeout (max 30min, size-adaptive) | [x] | crates/tee/src/limits.rs (compute_global_timeout: min(30min, 60s + size/64KB/s), 3 tests) + crates/tee/src/resource_pool.rs (Ticket::extend checks created_at against global_timeout) | 09 |
| §4.4 | Decode memory protection (header-based estimation) | [x] | crates/tee/src/limits.rs (estimate_decoded_size, 3 tests) + crates/tee/src/resource_pool.rs (Ticket::validate_decoded_size, 2 tests) | 09 |

### 5. System Implementation (§5)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §5.2 | TEE startup sequence (key generation → self-attestation → server start) | [x] | crates/tee/src/main.rs (runtime selection → KeyBundle generate → SolanaSigningKey generate → ProcessorRegistry → ResourcePool → self-attestation capture (fatal on failure) → Axum :4000) + crates/tee/src/server.rs (TeeAppState with attestation_verifier + expected_measurement, router: 6 endpoints, 9 tests) | 02, 13, post-15 |
| §5.2 | Mock TeeRuntime | [x] | crates/tee/src/runtime/mock.rs (MockRuntime: OsRng random_bytes, "mock-attestation:" attestation, 7 tests) gated by `runtime-mock` feature | 13 |
| §5.2 | AWS Nitro TeeRuntime | [x] | crates/tee/src/vendor/aws.rs (NitroRuntime via aws-nitro-enclaves-nsm-api 0.4: nsm_init, GetRandom loop, Attestation request, FakeNsm test backend, 4 tests). Build: `cargo build --features title-tee/vendor-aws` | post-15 |
| §5.2 | TEE HTTP endpoints | [x] | crates/tee/src/server.rs (GET /health, /keys, /processors, /solana-keys; POST /process, /extension/solana; spawn_blocking for sync orchestrator, 9 tests) | 13 |
| §5.2 | TEE request processing flow | [x] | crates/tee/src/orchestrator.rs (process_request: fetch → signature_hash → processors → JCS → attestation → ProcessResponse, 9 tests) | 04 |
| §5.2 | Content fetch: single (HTTP GET + ETag) | [x] | crates/tee/src/content_fetch.rs (HttpContentFetcher + ContentFetcher trait, 3 tests). Range Request streaming sandbox verified in task 01, sandbox tree removed post-verification | 01, 04 |
| §5.2 | Content fetch: fragmented | [x] | crates/tee/src/content_fetch.rs (init + segments concatenation, 3 tests). Fragment sandbox verified in task 01, removed post-verification | 01, 04 |
| §5.2 | Content fetch: sidecar | [x] | crates/tee/src/content_fetch.rs (manifest + content separate fetch, 3 tests) + crates/core/src/c2pa_verify.rs (compute_signature_hash_from_manifest_data) | 04 |
| §5.3 | Gateway: client auth + rate limiting | [x] | crates/gateway/src/auth.rs (ApiKeySet + Bearer token middleware) + crates/gateway/src/rate_limit.rs (token bucket per API key, 4 tests) + crates/gateway/src/server.rs (middleware layer, 5 auth tests + 1 rate limit test) | 10 |
| §5.3 | Gateway: TEE info relay | [x] | crates/gateway/src/state.rs (TeeInfoCache with RwLock, refresh_tee_info) + crates/gateway/src/endpoints.rs (cached responses for /keys, /processors, /health, /solana-keys) | 10 |
| §5.3 | Gateway: request proxy | [x] | crates/gateway/src/tee_client.rs (TeeClient trait + HttpTeeClient) + crates/gateway/src/endpoints.rs (handle_process, handle_solana_extension: relay to TEE) | 10 |
| §5.3 | Gateway: TEE restart detection + key refresh | [x] | crates/gateway/src/state.rs (check_and_refresh: polls TEE health, detects key change or recovery, refreshes cache; spawn_health_check background task, 2 tests) | 10, 14 |
| §5.3 | Gateway: binary + startup sequence | [x] | crates/gateway/src/main.rs (env config → HttpTeeClient → GatewayConfig → server::run; TEE_ENDPOINT, API_KEYS env vars) + crates/gateway/Cargo.toml ([[bin]]) | 14 |
| §5.3 | Gateway ↔ TEE HTTP integration (HttpTeeClient) | [x] | crates/gateway/src/tee_client.rs (HttpTeeClient: reqwest-based, 6 endpoints) + crates/gateway/tests/e2e.rs (8 E2E tests: health, keys, processors, solana-keys, process signed/unsigned, API key auth, TEE restart detection) | 10, 14 |
| §5.4 | Reproducible build (Dockerfile, Cargo.lock, toolchain pinning) | [x] | docker/tee-mock.Dockerfile + docker/gateway.Dockerfile (multi-stage, rust:1.93-bookworm → debian:bookworm-slim) + docker-compose.yml (TEE healthcheck → Gateway depends_on) + rust-toolchain.toml (1.93.1) + docker/smoke-test.sh (5 checks) | 15 |

### 6. Extension (§6)

| Section | Spec Item | Status | Implementation | Task |
|---|---|---|---|---|
| §6.1 | Extension framework (core result → extension request) | [x] | crates/solana/src/extension.rs (ExtensionRequest, OffchainData, process_extension: verify attestation → build cNFT TX → partial sign → serialize, 9 tests) | 12 |
| §6.2 | Solana Extension: Ed25519 signing key generation | [x] | crates/solana/src/signing_key.rs (SolanaSigningKey: generate, pubkey/pubkey_base58/pubkey_hash, sign, sign_transaction, 6 tests) | 12 |
| §6.2 | Solana Extension: Attestation Document for signing key | [x] | crates/solana/src/signing_key.rs (pubkey_hash: SHA-256(pubkey) for user_data) + crates/solana/src/extension.rs (verify_attestation_binding: JCS hash matching, mock + production paths, 3 tests) | 12 |
| §6.2 | Solana Extension: ZK proof generation (SP1 zkVM) | [~] | sp1-guests/attestation-aws-nitro/{program,host}/ (production guest + host CLI: cert chain verify in zkVM, Groth16 proof gen via `prove` binary, vkey extraction via `vkey` binary; Groth16 ~479 B fits Solana TX 1,232 B). Proof generation runs externally (~90 min CPU, ~30 GiB RAM) | 01, post-15 |
| §6.2 | Solana Extension: Whitelist PDA + four-step register_key verification | [x] | programs/title-whitelist/ (Anchor program: RegisterKey with vkey allowlist + SP1 Groth16 verification + measurement allowlist + user_data binding; ApprovedVkeys & ApprovedMeasurements PDAs; vendor-neutral StoredMeasurement type; devnet program ID `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`, 10 devnet integration tests including new initialize/add helpers) + crates/solana/src/whitelist.rs (client-side mirrors + derive_approved_vkeys_pda + derive_approved_measurements_pda, 8 tests) | 12, post-15 |
| §6.2 | Solana Extension: Developer collection setup + delegate | [x] | crates/solana/src/cnft.rs (build_mint_v2_ix: Optional core_collection/collection_authority/mpl_core_cpi_signer, 2 tests). Collection is developer's choice, not part of trust model | 12 |
| §6.2 | Solana Extension: cNFT mint (partial signing) | [x] | crates/solana/src/cnft.rs (build_create_tree_tx, build_mint_v2_ix, build_v0_tx, build_and_sign_mint_tx, serialize_transaction, 6 unit tests + devnet e2e: tree creation → cNFT mint → on-chain verify) | 12 |
| §6.2 | Solana Extension: Signing key expiry (90-day rotation) | [x] | programs/title-whitelist/ (KEY_EXPIRY_SECONDS, expires_at set on registration) + crates/solana/src/whitelist.rs (is_valid_at/is_expired_at, 2 tests) | 12 |
| §6.2 | Solana Extension: Whitelist key revocation (emergency, replay-resistant) | [x] | programs/title-whitelist/ (RevokeKey instruction: admin-only `revoked = true` flag flip, PDA stays in place so the original proof cannot re-create the entry; `WhitelistEntry.revoked: bool` field with `AlreadyRevoked` guard; KeyRevoked event) + crates/solana/src/whitelist.rs (`WhitelistEntry.revoked` mirrored, `is_valid_at` treats revoked as invalid, `WhitelistInstruction::RevokeKey`); devnet redeployed and revoke_key tests pass | 12, post-15 |
