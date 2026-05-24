# Audit I — Test Quality

Range audited: `crates/*/src/**/*.rs` (`#[cfg(test)] mod tests`), `crates/*/tests/*.rs`, `programs/title-whitelist/src/lib.rs`, `sp1-guests/**/*.rs`, `docker/smoke-test.sh`, `deploy/aws/scripts/*.sh`.

Spec read: `docs/v0.1.2/SPECS_JA.md` (§0–§6.2). Cross-checked which spec items have/lack assertions, and looked for "test name says X but assertion does not enforce X" patterns.

## Totals

| severity | count |
|---|---|
| must-fix  | 9 |
| should-fix | 10 |
| nitpick   | 5 |
| **total** | **24** |

## must-fix

### MF-1 — `rejects_invalid_bytes` has no assertion

`crates/attestation-aws-nitro/src/lib.rs:115-119`

```rust
#[test]
fn rejects_invalid_bytes() {
    let v = AwsNitroVerifier::new();
    let err = v.verify(b"not a valid attestation", 0).unwrap_err();
    matches!(err, AttestationError::ParseFailed(_));   // <-- no assert!
}
```

The `matches!(...)` invocation is a bare expression whose `bool` result is discarded. The test only confirms that *some* error is returned; it never confirms that the right error variant is returned. A regression where `verify()` returned `CertChainInvalid` for malformed bytes would silently pass.

Fix: wrap in `assert!(matches!(err, AttestationError::ParseFailed(_)), "{:?}", err);`. Same pattern should be audited everywhere the codebase uses `matches!` standalone (grep is clean elsewhere — only this one site is broken).

### MF-2 — AWS Nitro attestation verifier has only one fixture-based happy-path test

`crates/attestation-aws-nitro/src/lib.rs:124-140`, plus zero tests in `cose.rs`, `cert.rs`, `sign.rs`, `doc.rs`.

The entire vendor verification pipeline (COSE_Sign1 parsing, certificate chain traversal, ECDSA verification, leaf-signed payload, validity-period checks) has exactly one positive test against `tests/fixtures/attestation_1.report`. There is **no** test for:

- A tampered payload (1-bit flip in the COSE payload → must reject with `SignatureInvalid`).
- A tampered signature (1-bit flip in the COSE signature → must reject).
- A tampered certificate (1-bit flip in any cabundle cert → must reject with `CertChainInvalid`).
- An expired leaf or intermediate certificate (`check_ts` past validity → must reject with `Expired`).
- A `cabundle` with a foreign root (e.g. attacker-generated chain not rooted in AWS Nitro G1 — see SPEC §1.2 "ベンダールート証明書の信頼起点").
- Missing `pcrs[0]` (`MissingField("PCR0")`).
- The second fixture `attestation_2.report` (present in the tree, never loaded).

Spec §1.2/§5.2 entirely depend on this verifier being correct. Without negative tests, a regression that disables signature verification altogether would still pass `verifies_real_aws_nitro_attestation` (a parsed document with valid PCR length is enough). This is the highest-risk gap in the entire test suite.

Fix: add negative tests that mutate bytes of `attestation_1.report` at the COSE payload offset, the signature offset, and inside each cert in the cabundle, asserting on the precise error variant. Also test the spec-mandated AWS Nitro root pinning (the verifier currently "trusts the cabundle implicitly" — see lib.rs:47-51; if pinning is intentionally deferred, document it as a known gap in the spec rather than leaving an untested critical path).

### MF-3 — Solana on-chain program has zero tests

`programs/title-whitelist/src/lib.rs` (728 lines, 0 `#[test]`).

The program implements `register_key`, `revoke_key`, the two-tier `verifying_key_hash` / `measurement` check (SPEC §6.2 "二段の同一性確認"), and the "PDA stays with revoked flag to prevent proof replay" mechanism. None of this is exercised by any in-tree unit or integration test that runs without a live devnet endpoint.

`crates/solana/tests/devnet_whitelist.rs` *does* test these paths, but every single one of its 9 tests is `#[ignore]`. A normal `cargo test` run gets zero coverage of:

- `register_key` rejects unapproved `verifying_key_hash` (§6.2 確認1)
- `register_key` rejects unapproved `measurement` (§6.2 確認2)
- `register_key` rejects `user_data` not matching `SHA-256(signing_pubkey)` (§6.2 確認3 — bind 確認)
- `revoke_key` flips the flag without closing the PDA (§6.2 — PDA 存続要件)
- Re-registering a revoked key fails (the structural guarantee SPEC §6.2 explicitly calls out)

Fix: integrate `litesvm` or `solana-program-test` to run the program in-process. The current devnet-only flow means a) the protocol's central trust mechanism is unverifiable in CI, and b) external contributors cannot run the tests at all without provisioning devnet SOL.

### MF-4 — Devnet integration tests are CI-invisible (`#[ignore]` × 9)

`crates/solana/tests/devnet_whitelist.rs:148, 161-194, 233-251, 253-284, 286-332, 338-447, 469-503, 508-553, 557-601`.

Every test in the file is `#[ignore]`d. The header comment explains the run command but there is no scheduled job, no Makefile target, no CI workflow that ever passes `--ignored`. Consequently:

- `register_key_rejects_invalid_proof` — sets `sp1_vkey_hash = [0; 32]`, which is rejected at the *vkey-not-approved* check before the proof verifier ever runs. The error this test "expects" is not the error its name implies — it never actually exercises SP1 proof rejection.
- `register_key_rejects_empty_proof` — same issue, fails at vkey check.
- `revoke_key_rejects_nonexistent_pda` and `revoke_key_rejects_non_admin` — only enforce error-on-failure, but `err_msg.contains("Error")` is so permissive that a 100% unrelated failure (e.g. RPC rate limit, expired blockhash, network DNS) would still pass.
- `initialize_registries_devnet` / `add_placeholder_vkey_devnet` / `add_placeholder_measurement_devnet` — these are *operational* one-shots dressed up as tests; mixing them into `cargo test --ignored` will deterministically fail on a fresh deployment (one is "already in use" once run, and re-runs are expected to fail with `0x1775` / `0x1778`). They belong in a separate `xtask` or shell script, not in `tests/`.

Fix: decide for each test whether it (a) belongs in CI under `--ignored` with secret-based devnet credentials, (b) should be ported to local LiteSVM (the must-fix above), or (c) is operational tooling and should not live under `tests/`. Strengthen the loose `contains("Error")` assertions to match specific Anchor error codes.

### MF-5 — `decrypt` AEAD test omits all tampering cases

`crates/crypto/src/aead.rs:67-92`.

The AES-256-GCM tests cover only `encrypt_decrypt_roundtrip` and `wrong_key_fails`. The whole point of the AEAD authentication tag is to detect ciphertext mutation and nonce reuse — neither is tested:

- 1-bit flip in `ciphertext[mid]` → must return `DecryptError`.
- 1-bit flip in the authentication tag (last 16 bytes) → must return `DecryptError`.
- Wrong `nonce` (correct key, ciphertext, but different nonce) → must return `DecryptError`.
- Truncated ciphertext (length < tag size) → must return `DecryptError`.

The AEAD layer is the spec §2.4 ciphertext-integrity primitive; a regression that switched to CBC (no tag) would pass the current tests.

### MF-6 — `sealed_channel` tests omit tampering across the integrated flow

`crates/crypto/src/sealed_channel.rs:99-216`.

Six roundtrip tests; only `wrong_bundle_fails` is a negative case. Missing:

- Tamper with `wire[suite_id_byte]` → expect `UnsupportedSuite` or wrong-suite mismatch.
- Tamper with `wire[encap_key]` (1-bit flip in the ephemeral pubkey) → expect HKDF derives wrong key → `DecryptError`.
- Tamper with `wire[nonce]` → expect `DecryptError`.
- Tamper with `wire[ciphertext]` → expect `DecryptError`.
- Reuse the same encap_key across two requests but with mutated ciphertext (replay-with-mutation) → expect `DecryptError`.

These five are the exact attack model §2.4 documents (Gateway is untrusted and "can物理的に持つ" the ability to mutate). Without them the test suite gives a false sense of security.

### MF-7 — `verify_attestation_binding_measurement_match` tests against the all-zero placeholder

`crates/solana/src/extension.rs:284-292`, with `MockAttestationVerifier::MEASUREMENT = [0u8; 48]` (`crates/attestation/src/lib.rs:114`).

The "measurement matches" test compares against a constant of 48 zero bytes — a value that would never appear on a real Nitro PCR0. Worse, the global `expected_measurement` field in `TeeAppState` is initialised to the same all-zero buffer in every test path (`gateway/tests/e2e.rs:79`, `tee/src/server.rs:332`). This means:

- A bug where the verifier returns `measurement: vec![0u8; 48]` regardless of input would pass.
- A bug where `process_extension` skipped the measurement comparison entirely (or compared against the wrong field) would also pass — because `MockAttestationVerifier` always emits zeros, and the expected value is always zeros.

Fix: parameterise `MockAttestationVerifier` to take an arbitrary measurement at construction, and have the extension tests assert that swapping in a non-zero measurement is correctly bound. The spec §6.2 "二段の同一性確認 — 確認2" is the on-chain trust pivot; its mock-side equivalent must actually distinguish values.

### MF-8 — KEM tests verify only roundtrip, not tampering or cross-suite confusion

`crates/crypto/src/kem/x25519.rs:84-110`, `kem/p256_ecdh.rs:90-112`, `kem/ml_kem768.rs:91-110`.

Per KEM: one `roundtrip` test and one length/uniqueness test. Missing for all three:

- Truncated/zero-length `encap_key` → expect `InvalidKeyLength` (some lengths are actually tested via wire.rs but not at KEM layer).
- For X25519: the standard "all-zero point" small-subgroup case (x25519-dalek already rejects it, but the test should pin that behaviour as a Title Protocol invariant).
- For P-256: an off-curve `encap_key` (a SEC1-formatted 65-byte string whose point fails the curve equation) → expect `InvalidKeyLength`/`Decrypt`-like error. `PublicKey::from_sec1_bytes` is supposed to validate this; nothing pins the contract.
- For ML-KEM-768: pin the implicit-rejection behaviour (a random `encap_key` of correct length still "succeeds" but produces a useless shared secret — the AEAD layer should fail; the test suite never demonstrates this).
- Cross-suite confusion: feed an X25519 encap_key (32 bytes) to a P-256 decapsulator — already prevented by length check in the wire layer, but worth pinning at the `Decapsulator` impl too.

Fix: add per-KEM negative tests covering format-invalid inputs and (for ML-KEM) implicit-rejection chaining with AEAD failure.

### MF-9 — `pipeline_unsigned_content_rejected` returns wrong error class but test is silent about it

`crates/tee/src/orchestrator.rs:699-728`.

The test asserts `matches!(result.unwrap_err(), OrchestratorError::SignatureHashFailed(_))`, which is correct. But the spec §3.1 quoted in code at line 73 says **"C2PA署名のないコンテンツに対してはリクエスト全体が拒否される"** — that *should* be its own error variant (`MissingC2paSignature`) so the Gateway can map it to a specific 4xx response, not the catch-all `SignatureHashFailed("...")` which is also used for I/O failures in the JUMBF parser. The test cements a sloppy error taxonomy.

Fix at the test level: add a separate test that exercises a *different* failure mode of `compute_signature_hash` (e.g. corrupted JUMBF that *does* claim to be C2PA-signed) and asserts they produce distinguishable errors. If they don't, the test surfaces a real spec→implementation gap that error handling audit C should pick up.

## should-fix

### SF-1 — `process_signed_content` accepts either "valid" or "invalid" validation

`crates/core/src/c2pa_verify.rs:400-404`:

```rust
assert!(
    output.validation == "valid" || output.validation == "invalid",
    ...
);
```

That's "any string from the two-element vocabulary" — exactly the assertion shape spec §3.2 already guarantees by typing. The comment ("Self-signed cert → ValidationState may be Invalid... depending on c2pa-rs version") admits the test is hedging against upstream behaviour drift. Pick one path: either pin a trust list and require `"valid"`, or assert `output.validation == "invalid"` and explain why. The current shape would pass even if the processor returned `"valid"` for an unsigned image.

### SF-2 — `signature_hash_differs_for_different_content` actually tests "signing twice produces different hashes"

`crates/core/src/c2pa_verify.rs:487-501`. The test name promises "different content → different hashes," but both inputs are calls to `create_signed_jpeg()` on the *same* pixel data — they differ only because EphemeralSigner randomises the certificate per signing. So this verifies cert-determinism of `signature_hash`, not content-sensitivity. A real content-sensitivity test would sign two different images. Rename to `signature_hash_differs_for_different_signing_events`, and add a separate test that signs two different images with the same context to demonstrate content sensitivity.

### SF-3 — `MockRuntime` re-implementations are duplicated three times

`crates/tee/src/lib.rs:99-143` (one MockRuntime), `crates/tee/src/runtime/mock.rs:22-122` (another MockRuntime — the real one), `crates/tee/src/orchestrator.rs:421-454` (a third, scoped to tests), plus the inline `MockTeeClient` in `gateway/src/server.rs:160-277`. The lib.rs one returns user_data verbatim (no prefix), runtime::mock prepends `"mock-attestation:"`, and orchestrator's uses a `Mutex` to record `last_user_data`. Tests written against one of these would not catch behaviour required by another. Consolidate to one `MockRuntime` definition; add a `record_calls` toggle for the orchestrator's needs.

### SF-4 — `random_bytes` tests assert length only

`crates/tee/src/lib.rs:137-142`, `crates/tee/src/vendor/aws.rs:197-202`. Both return all-zero buffers under their `Mock`/`Fake` NSM and the test only verifies `len == 32`. The runtime::mock variant *does* have `random_bytes_not_all_zero` (line 99-104) — that's the right pattern; copy it to the other two.

Also: `RealNsm::get_random` loops because `Request::GetRandom` returns ≤256 bytes per call. The "loop until satisfied" branch is untested. `FakeNsm` always returns the full requested length in one call. Add a `FakeNsm` variant that returns short reads (e.g. 100 bytes per call) and request 300 bytes to exercise the loop and prove it doesn't infinite-loop on partial responses.

### SF-5 — Timing-based tests are flaky candidates

`crates/tee/src/resource_pool.rs:560-617` use `thread::sleep(Duration::from_millis(1..30))` to drive timeout checks. Under loaded CI runners (or macOS with high-precision scheduler latency) these can flap either direction. Same risk in `gateway/src/server.rs:680-715` (depends on `tokio::time::sleep(100ms)` for restart detection) and `gateway/tests/e2e.rs:402-421` (2 s sleep waiting for health-check refresh — slow *and* flaky).

Fix: use `tokio::time::pause()` + `advance()` for tokio paths; for resource_pool, expose an injectable `now: impl Fn() -> Instant` so tests drive time directly.

### SF-6 — Fragmented memory pattern test pins suboptimal behaviour

`crates/tee/src/content_fetch.rs:686-741`. Spec §4.3 prescribes `ticket.extend(fragment) → process → ticket.shrink(fragment)`, with peak memory = init + 1 fragment. The current implementation concatenates everything (file comment line 397-403 explicitly admits "future optimization"). The test asserts `ticket.reserved() == init + seg0 + seg1` — i.e. it pins the *non-spec* behaviour. If somebody fixes the implementation, the test fails. Either:

- (a) Add a `// known deviation from §4.3, see issue #X` comment to the test and the implementation, so the future fixer knows to update the test, OR
- (b) Drop the exact-bytes assertion and only verify content equality + that ticket is non-zero.

Currently the test silently rewards staying out of spec.

### SF-7 — Several spec limits have no end-to-end enforcement test

| Limit | Spec | Tested? |
|---|---|---|
| Fragment count > 100 000 → reject | §4.4 | Only at `limits.rs:213-223` unit level; never via `fetch_content` |
| `chunk timeout = 60s` between fetches | §4.4 | Only resource_pool unit test with 1 ms timeout; never with the actual `CHUNK_TIMEOUT` constant |
| `global timeout` = `min(MAX, base + size/speed)` capped at 30 min | §4.4 | Constants tested at limits.rs but never via a real request |
| Provenance graph 10 000 node+edge cap | §4.4 | No processor for this is implemented, so no test. If processor is shipped later this must come with the cap test. |
| `etag` / `If-Match` mid-fetch change → 412 | §5.2 | `MockFetcher` always sets `etag: Some("mock-etag")` but no test re-fetches with a changed body |

Fix: at least add `fetch_content` tests that wire the limits in with the real constants (use a `MockFetcher` that returns 100 001 segments).

### SF-8 — Gateway `MockTeeClient::process` ignores the request body

`crates/gateway/src/server.rs:243-255`. It returns a constant `ProcessResponse` (signature_hash = `"sha256:mock"`) regardless of input. The tests `process_relays_to_tee`, `process_with_auth`, etc. assert on this constant. So nothing in the Gateway test suite verifies that the request body is actually forwarded to the TEE intact — a bug where `handle_process` dropped the body and sent `{}` would pass.

Fix: have `MockTeeClient` echo back the input's `content_url` inside the response somewhere, and assert on that.

### SF-9 — `auth.rs` constant-time `contains` is undertested

`crates/gateway/src/auth.rs:98-116`. The comment promises XOR-accumulator, no short-circuit on first match, fixed-time per-entry. Tests only assert presence/absence (lines 123-137). Without a statistical timing test (or at minimum a comment-anchored property test that mutates the candidate position-by-position), regressions back to `HashSet::contains` would silently pass. The comment even admits "length-mismatched entries leak via execution time" — that *should* be a documented test.

Add at least one test that exercises the corner cases (empty candidate, candidate longer than every stored key, candidate sharing a prefix with a stored key) so the algorithm shape is pinned even if a runtime check can't be deterministic.

### SF-10 — `solana_extension_rejects_bad_pubkey` is the only `/extension/solana` server test

`crates/tee/src/server.rs:486-501`. The happy-path of `POST /extension/solana` (valid pubkeys, valid offchain data URL → fetches → verifies attestation → constructs partial tx → returns base64) is exercised nowhere in `server.rs`. Coverage exists at `extension.rs:294-314` (`process_extension_full_pipeline`) for the orchestration layer, but the HTTP handler that bridges request JSON ↔ `ExtensionRequest` ↔ `process_extension` is only tested for the `BAD_REQUEST` error path.

Add a happy-path server test using `MockAttestationVerifier` + a fake fetcher for the offchain URL.

## nitpick

### N-1 — Bilingual test commentary

Most tests use English (`fn rejects_invalid_proof`); `crates/core/src/processor.rs:156-198` uses Japanese in comments (`/// テスト用のモックprocessor`, `/// Processor trait がオブジェクトセーフ`). `crates/tee/src/lib.rs:99-142` is the same. Pick one for tests project-wide — Japanese is fine for doc comments on production code, but mixing inside one `#[cfg(test)] mod` makes greppability worse.

### N-2 — `processor_trait_object_safety` and `trait_object_safety` are name-collision pairs

`crates/core/src/processor.rs:191-199` (`processor_trait_object_safety`) and `crates/tee/src/lib.rs:122-127` + `crates/tee/src/runtime/mock.rs:114-122` (both `trait_object_safety`). Tests with the same name across crates show up confusingly in `cargo test`'s output. Prefix with the trait name when reused.

### N-3 — Assert messages absent in most tests

`crates/crypto/src/aead.rs:78`: `assert_ne!(ciphertext, plaintext);` — when this fails the diagnostic is "left != right" on hex blobs. Compare to `crates/tee/src/orchestrator.rs:820-823` which provides context strings. Project-wide, ~80 % of asserts have no message. Not actionable as one-off fixes, but worth a project policy.

### N-4 — `cnft_mint_tx_construction` mixes `#[ignore]` despite being a unit-style test

`crates/solana/tests/devnet_whitelist.rs:286-332`. Most of this test is pure local construction (build_mint_v2_ix, sign, serialise, assert size ≤ 1232) — *no devnet RPC is actually required* except the call at line 297 to get a blockhash. Replace `client.get_latest_blockhash()` with `Hash::new_unique()` and drop the `#[ignore]` to get instant CI coverage of the cNFT TX layout invariant (which is a hard Solana constraint).

### N-5 — `fetch_fragmented_fragment_size_exceeded` doesn't test what it claims

`crates/tee/src/content_fetch.rs:824-855`. The test name implies "exceeded → reject" but the body uses a 10-byte fragment and asserts success (line 854: `assert!(result.is_ok())`). The comment even concedes "we can't create a 100MB+ vec in tests, so this test verifies wiring." Rename to `fetch_fragmented_small_fragment_passes_validation` and leave a separate "exceeded path is tested in `limits::tests`" note in the doc.

## Cross-cutting observations (not numbered)

1. The test suite is heavy on **schema-roundtrip tests** (`crates/core/src/{request,response,processor_outputs}.rs` have ~15 of them) that are valuable for spec stability but cover none of the security properties. The asymmetry — extensive serde tests, near-zero tampering tests — is the dominant pattern of the audit.

2. **No SP1 guest tests exist** in `sp1-guests/attestation-aws-nitro/program/` (only `main.rs`, no `#[cfg(test)]`). For something whose entire purpose is to produce a `verifying_key_hash` that gets pinned on-chain, the guest program should at minimum have a smoke test that proves the same input always yields the same vkey hash (otherwise a host-side build change silently invalidates the on-chain registry).

3. **Smoke test (`docker/smoke-test.sh`) only exercises GETs**. POST /process (with a real signed JPEG fixture) and POST /extension/solana are never smoke-tested. The "stack starts cleanly" guarantee says nothing about the actual contract.

4. **Test naming inconsistency** — `c2pa_verify_added_when_missing` (snake-case verb-first), `whitelist_entry_validity` (noun), `pipeline_single_content_success` (compound), `e2e_process_signed_content` (prefix-then-verb). Within one file the style is consistent; across the workspace it's three competing conventions.
