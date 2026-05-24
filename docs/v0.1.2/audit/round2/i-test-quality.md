# Audit I (Round 2) — Test Quality

Re-audit scope identical to Round 1: `crates/*/src/**/*.rs` (`#[cfg(test)] mod tests`), `crates/*/tests/*.rs`, `programs/title-whitelist/src/lib.rs`, `sp1-guests/**/*.rs`, `docker/smoke-test.sh`, `deploy/aws/scripts/*.sh`.

Spec re-read: `docs/v0.1.2/SPECS_JA.md` (§0–§6.2).

## Round 1 指摘の処理状況

| status | count |
|---|---|
| fixed | 2 |
| partially-fixed | 5 |
| unchanged | 17 |
| regressed | 0 |
| **total** | **24** |

(Cross-cutting observations: 1 partial / 3 unchanged.)

### fixed (2)

- **MF-1** `rejects_invalid_bytes` — `crates/attestation-aws-nitro/src/lib.rs:95` now uses `assert!(matches!(err, AttestationError::ParseFailed(_)))`. The bare `matches!` expression that discarded its bool result is gone.
- **SF-3 (lib.rs MockRuntime only)** — `crates/tee/src/lib.rs:55-67` was rewritten as a `StubRuntime` shape used only inside that crate's own `#[cfg(test)]` module. The duplicate "no-prefix" MockRuntime that lived at the crate root has been replaced. Still partially open because two `MockRuntime` types remain (see below).

### partially-fixed (5)

- **MF-5** `decrypt` AEAD tampering — `crates/crypto/src/aead.rs:101-107` adds `wrong_aad_fails`. Still missing: 1-bit flip in ciphertext middle, 1-bit flip in the GCM tag, wrong-nonce-same-key, and truncated-below-tag-size cases. The spec §2.4 ciphertext-integrity primitive remains under-tested.
- **MF-7** measurement mock binding — `crates/solana/src/extension.rs:250-257` added `verify_attestation_binding_measurement_mismatch` using `[0xAA; 48]`, a non-zero expected value that does distinguish from the mock's all-zero output. Still partial because `verify_attestation_binding_measurement_match` (line 260-267) and the suite-wide `expected_measurement` (`tee/src/server.rs:376`, `gateway/tests/e2e.rs:79`) all keep using the all-zero `MockAttestationVerifier::MEASUREMENT` constant. The mock has not been parameterised — a regression that hard-codes `vec![0u8; 48]` in the verifier would still pass every "match" test.
- **SF-2** `signature_hash_differs_for_different_content` — `crates/core/src/c2pa_verify.rs:526-539`: the in-test comment is now accurate ("Two separately signed copies of the same image have different signatures") and the failure message reads "Different signing events should produce different signature_hashes". The function name was *not* renamed, so the test still claims to verify content-sensitivity it never exercises, and no companion test signs two distinct images.
- **SF-3** MockRuntime duplication — lib.rs version gone (see "fixed"), but `crates/tee/src/runtime/mock.rs:22-122` and `crates/tee/src/orchestrator.rs:441-454` still both define `MockRuntime` with different fields (the orchestrator copy carries a `Mutex<Option<Vec<u8>>>` recorder, the runtime/mock.rs copy does not). Behaviour was harmonised — both now prefix `"mock-attestation:"` — but the type-name collision remains and the recorder is still scoped to the orchestrator file only.
- **SF-4** `random_bytes` length-only assertion — `crates/tee/src/vendor/aws.rs:182-184` FakeNsm now returns `0xAB` rather than zero, and `crates/tee/src/lib.rs:84-86` stubs out with zeros (no `not_all_zero` pin). The partial-read loop in `RealNsm::get_random` (the 256-byte chunked branch) is still untested — no FakeNsm variant returning short reads.

### unchanged (17)

- **MF-2** AWS Nitro negative paths — `crates/attestation-aws-nitro/src/{cose,cert,sign,doc}.rs` still hold zero `#[test]`. No tampered-payload / tampered-signature / tampered-cert / expired-cert / foreign-root / missing-PCR0 tests against `tests/fixtures/attestation_1.report`; `attestation_2.report` is still never loaded. The spec §1.2 root-pinning chain is exercised only by the single happy-path `verifies_real_aws_nitro_attestation`.
- **MF-3** Solana on-chain program has zero tests — `programs/title-whitelist/src/lib.rs` still has no `#[cfg(test)]`. No `litesvm` / `solana-program-test` was introduced (verified by grep across `programs/` and `crates/solana`). The §6.2 三段の同一性確認 trust pivot remains unverifiable in `cargo test`. (Note: `crates/solana/src/whitelist.rs` did gain client-side mirror tests for `WhitelistEntry::is_valid_at`, expiry constant, PDA derivation — those don't substitute for testing the on-chain handler logic.)
- **MF-4** Devnet integration tests all `#[ignore]` — `crates/solana/tests/devnet_whitelist.rs` still has 9 `#[test] #[ignore]` blocks (lines 148, 161, 193, 233, 253, 286, 338, 468, 508, 557). No CI job, no Makefile target, no `--ignored` runner; loose `err_msg.contains("Error")` assertions still in place; the `initialize_registries_devnet` / `add_placeholder_*` one-shots still live under `tests/` with no separation.
- **MF-6** `sealed_channel` integrated-flow tampering — `crates/crypto/src/sealed_channel.rs:110-224` still has six roundtrip tests and only `wrong_bundle_fails` as negative. No bit-flip tests against `wire[suite_id_byte]`, `wire[encap_key]`, `wire[nonce]`, `wire[ciphertext]`, no replay-with-mutation case. The §2.4 Gateway-untrusted threat model has the same false-sense-of-security gap.
- **MF-8** KEM per-suite negative tests — `crates/crypto/src/kem/{x25519,p256_ecdh,ml_kem768}.rs` still each have exactly two tests (`roundtrip`, `each_encapsulation_unique` / length pinning). No off-curve P-256 inputs, no X25519 small-subgroup point, no ML-KEM-768 implicit-rejection chained through AEAD, no cross-suite confusion at the `Decapsulator` impl boundary.
- **MF-9** `pipeline_unsigned_content_rejected` error taxonomy — `crates/tee/src/orchestrator.rs:719-748` unchanged; still maps unsigned content to `OrchestratorError::SignatureHashFailed(_)` (same variant as I/O failures in the JUMBF parser). No `MissingC2paSignature` variant introduced, no companion test that pits a JUMBF-corrupted-but-claims-signed input against an unsigned input to demonstrate the rejections are distinguishable.
- **SF-1** `process_signed_content` accepts either "valid" or "invalid" — `crates/core/src/c2pa_verify.rs:438-442` still has `output.validation == "valid" || output.validation == "invalid"`. Neither trust list pinning nor a hard `assert_eq!` direction was chosen.
- **SF-5** Sleep-based timing tests — `crates/tee/src/resource_pool.rs:548, 565, 595` still use `thread::sleep`; `crates/gateway/tests/e2e.rs:404 (100 ms)` and `:422 (2 s)` unchanged. No `tokio::time::pause()/advance()` migration. No injectable clock added to `resource_pool`.
- **SF-6** Fragmented memory pattern pins suboptimal behaviour — `crates/tee/src/content_fetch.rs:726-729` still asserts `ticket.reserved() == init + seg0 + seg1` (concatenation). No `// known deviation from §4.3` marker added; a future fix to the §4.3 `extend → process → shrink` pattern still silently fails this test.
- **SF-7** Spec limits with no end-to-end enforcement — `validate_fragment_count` is invoked at `content_fetch.rs:347`, but no `fetch_content`-level test passes 100_001 fragments. `CHUNK_TIMEOUT` is still only exercised at 1 ms in `resource_pool` (never with the real 60 s constant). `MockFetcher` still hard-codes `etag: None` or `Some("\"test-etag\"")` with no mid-fetch change → no 412 path test.
- **SF-8** Gateway `MockTeeClient::process` ignores request body — `crates/gateway/src/server.rs:262-275` still discards `_req` and returns the same hard-coded `ProcessResponse{ signature_hash: "sha256:mock", ... }`. A `handle_process` regression that swapped the body for `{}` still passes every Gateway test.
- **SF-9** `auth.rs` constant-time `contains` undertested — `crates/gateway/src/auth.rs:142-156` still has only `api_key_set_operations` and `empty_set_allows_all`. No empty-candidate / longer-than-stored / prefix-collision test for the XOR-accumulator algorithm shape.
- **SF-10** Only one `/extension/solana` server test — `crates/tee/src/server.rs:531-546` still only the `solana_extension_rejects_bad_pubkey` BAD_REQUEST path. No happy-path server test that drives the offchain-fetch → attestation-verify → partial-tx round trip through the HTTP boundary.
- **N-1** Bilingual test commentary — `crates/core/src/processor.rs:150, 172` still has `/// テスト用のモックprocessor` and the inline Japanese comment. `crates/tee/src/lib.rs` has been rewritten in English (mild improvement). Project-wide policy still absent.
- **N-2** Test-name collision — `crates/tee/src/lib.rs:70 (trait_object_safety)`, `crates/tee/src/runtime/mock.rs:115 (trait_object_safety)`, `crates/core/src/processor.rs:171 (processor_trait_object_safety)`. Three collisions in two name-shapes still surface confusingly in `cargo test` output.
- **N-3** Assert messages absent — sampled: `crates/crypto/src/aead.rs:85, 88, 98, 106` still bare `assert!` / `assert_eq!` / `assert_ne!`. No project-wide policy added.
- **N-5** `fetch_fragmented_fragment_size_exceeded` doesn't test exceeded — `crates/tee/src/content_fetch.rs:813-844` still uses a 10-byte fragment and asserts `result.is_ok()`. Name still misleads.

### regressed (0)

No previously-fixed test was broken or skipped.

## 新規発見 (Round 2 で見つけたもの)

### must-fix

#### R2-MF-1 — `prune_drops_full_idle_buckets` is a new flaky test (50 ms / 10 ms window)

`crates/gateway/src/rate_limit.rs:173-183`

```rust
let limiter = RateLimiter::new(5, 1);
assert!(limiter.check_rate_limit("k1"));
assert!(limiter.check_rate_limit("k2"));
thread::sleep(Duration::from_millis(50));
let pruned = limiter.prune_idle(Duration::from_millis(10));
assert_eq!(pruned, 2);
```

Loaded CI runners can pause longer than 50 ms inside `sleep`, but they can also resume *before* the 10 ms boundary on the `prune_idle` side if the clock source is coarser than expected. More importantly, the bucket refill rate (`1` token / second per the constructor) and the actually-elapsed wall clock both feed into whether each bucket counts as "full and idle". A 50 ms / 10 ms ratio is a 5× margin, which on CPU-contended GitHub-Actions runners can flake either direction. Round 1 SF-5 already called out timing-test flakiness in `resource_pool.rs` / `gateway/tests/e2e.rs`; Round 2 added a fresh instance instead of removing existing ones.

Fix: switch to an injectable clock (the same fix recommended in SF-5) so the test drives the boundary directly without `sleep`.

#### R2-MF-2 — `refills_over_time` adds a second new sleep-based flaky test

`crates/gateway/src/rate_limit.rs:162-171`. Same crate, same module, also new since Round 1. Refill window is 600 ms against a 1-token-per-second rate (60% of the refill window), which is a less-tight margin than R2-MF-1 but still wall-clock-dependent. Lift the clock-injection fix here too.

### should-fix

#### R2-SF-1 — `process_extension_rejects_tampered` only checks `is_err()`

`crates/solana/src/extension.rs:291-307`. A test added since Round 1 that mutates `response.verifiable.signature_hash` to `"sha256:tampered"` and asserts `result.is_err()`. The Round 1 critique of MF-9 applies: any error variant satisfies the assertion (RPC error, signing error, attestation parse error — all pass). The spec-implicit expectation is that user_data tamper detection yields a specific binding-mismatch class, not "any failure".

Fix: tighten to `assert!(matches!(result.unwrap_err(), ExtensionError::UserDataMismatch { .. }))` or whichever variant the implementation actually emits (and add it if missing).

#### R2-SF-2 — `fetch_fragmented_success` was extended to pin the SF-6 anti-pattern more strictly

`crates/tee/src/content_fetch.rs:712-729` now also asserts `result.content_bytes.len() == init + seg0 + seg1` *and* the per-segment slice positions (lines 719-723). The concatenation behaviour Round 1 flagged as non-spec is now pinned even more tightly — a future fix toward §4.3's `extend → process → shrink` peak-memory invariant breaks more assertions, not fewer.

Fix: same as SF-6 (add a `// known deviation` marker, or drop the byte-exact length and offset assertions in favour of content-equality + non-zero-reserved). The drift in *the wrong direction* makes this a fresh should-fix in addition to the unchanged SF-6.

#### R2-SF-3 — `cnft.rs` got the local-construction tests N-4 asked for, but the ignored `cnft_mint_tx_construction` was left in place

`crates/solana/src/cnft.rs:262-377` now has 7 fast local tests (`derive_tree_config_deterministic`, `build_mint_v2_ix_no_collection`, `build_v0_tx_basic`, `build_and_sign_mint_tx_applies_signature`, `serialize_transaction_roundtrip`, ...) — these cover the cNFT TX layout invariant that Round 1 N-4 wanted to lift out of devnet. Good. But the original `crates/solana/tests/devnet_whitelist.rs:286-332 cnft_mint_tx_construction` was **not** removed or repurposed. It is now redundant with the new in-crate tests and remains stuck behind `#[ignore]` + `get_latest_blockhash` call, contributing to the MF-4 surface area for no remaining benefit.

Fix: delete `cnft_mint_tx_construction` from `tests/devnet_whitelist.rs` now that `crates/solana/src/cnft.rs` covers it locally; or, if a devnet smoke-confirm is still wanted, swap `get_latest_blockhash()` for `Hash::new_unique()` so it can run unguarded (per the Round 1 N-4 fix recommendation).

### nitpick

#### R2-N-1 — Empty `MockTeeClient` mutex defaults silently fall through

`crates/gateway/src/server.rs:204-205` initialises `solana_keys_response` and `solana_ext_response` to `Mutex::new(None)`. Tests that forget `.with_solana()` get `Ok(None)` from `solana_keys()` and a 404-like behaviour from `solana_extension()` (depending on handler). This is intentional but undocumented; a future contributor reading a "solana endpoint returns nothing" test will not know the missing `.with_solana()` is the cause. A two-line module-level doc comment on the test-only `MockTeeClient` (or a `must_use_with_solana_or_returns_empty` rename) would prevent silent miswiring.

#### R2-N-2 — `MockProcessor` is duplicated across crates

`crates/core/src/processor.rs:150-168` and `crates/tee/src/orchestrator.rs:421-454` both define test-local `MockProcessor`-shaped helpers. Different shapes (one returns a `Result`, one is a `Processor` trait impl with hard-coded outputs). Round 1 SF-3 covered MockRuntime; this is the same pattern for Processor. Low impact today, but a shared `title-test-support` test-helper crate would eventually pay off as the test surface grows.

#### R2-N-3 — `wrong_aad_fails` has no assert message

`crates/crypto/src/aead.rs:106`: `assert!(decrypt(&key, &nonce, &ciphertext, b"wrong").is_err());`. The N-3 pattern applies — added since Round 1 but follows the same bare-assert style.

## Cross-cutting observations (Round 2 status)

1. **Schema-roundtrip-heavy vs tampering-light asymmetry** — unchanged. The crypto and attestation crates still trade in roundtrip and length-pinning tests; the security-property tests Round 1 enumerated (tampering, replay, cross-suite) were almost entirely not added. The one structural improvement is the `verify_attestation_binding_measurement_mismatch` addition in `solana/src/extension.rs:250`.

2. **No SP1 guest tests** — unchanged. `sp1-guests/attestation-aws-nitro/program/src/main.rs` and `sp1-guests/attestation-aws-nitro/host/src/lib.rs` still have no `#[cfg(test)]`. The `verifying_key_hash`-stability smoke test Round 1 asked for is still missing.

3. **Smoke test (`docker/smoke-test.sh`) only exercises GETs** — unchanged. Lines 49-53 still only `GET /health`, `/keys`, `/processors`, `/solana-keys`. No `POST /process` with a signed-JPEG fixture, no `POST /extension/solana`. The "stack starts cleanly" guarantee continues to say nothing about the actual contract.

4. **Test naming inconsistency** — slightly worse. New tests added in `crates/solana/src/cnft.rs:262-377` lean toward verb-first (`build_mint_v2_ix_no_collection`, `serialize_transaction_roundtrip`), but `crates/solana/src/whitelist.rs:124-201` introduced noun-first names (`whitelist_entry_validity`, `key_expiry_is_90_days`). New tests in `crates/gateway/src/rate_limit.rs` (`allows_within_limit`, `rejects_over_limit`, `refills_over_time`, `independent_per_key`, `prune_drops_full_idle_buckets`) adopt a fourth verb-third-person style. The project-wide convention drift Round 1 noted continued during Round 1→2 fixing.

## 集計 (Round 2)

| カテゴリ | Round 1 残 | Round 2 新規 | Round 2 合計 |
|---|---|---|---|
| must-fix    | 7 (MF-2/3/4/5/6/7/8/9 minus MF-1; MF-5/7 are partial) | 2 (R2-MF-1, R2-MF-2) | 9 |
| should-fix  | 9 (SF-1..10 minus SF-3 partial credit; SF-2/4 partial) | 3 (R2-SF-1, R2-SF-2, R2-SF-3) | 12 |
| nitpick     | 4 (N-1/2/3/5; N-4 partially addressed via cnft.rs but devnet test still there) | 3 (R2-N-1, R2-N-2, R2-N-3) | 7 |
| **計**     | **20** | **8** | **28** |

Net delta vs Round 1: +4 open items. Round 1 closed 7 outright/partially (MF-1, SF-3 lib.rs portion, MF-5 partial, MF-7 partial, SF-2 partial, SF-3 partial, SF-4 partial) but introduced 8 new ones during the fix passes, of which 2 are net-new must-fix flaky tests in the same family Round 1 already warned about (SF-5).

## 推奨される最優先対応 (top 5)

1. **MF-2** — AWS Nitro vendor verifier negative tests. Pure additive work; bytes already in the fixtures. Single highest-risk gap in the suite (whole §1.2 / §5.2 trust chain has no negative coverage).
2. **MF-3** — Introduce `litesvm` or `solana-program-test` for `programs/title-whitelist`. Without this the §6.2 三段の同一性確認 cannot be CI-verified, period.
3. **R2-MF-1 / R2-MF-2 + SF-5** — Pull all `thread::sleep` / `tokio::time::sleep` based timing tests onto an injectable clock. Currently the codebase has 6+ such tests and grew during the fix pass.
4. **MF-6** — Sealed-channel tampering matrix. Same shape work as MF-2; cheap to add, large security signal.
5. **SF-8 + SF-10** — Make `MockTeeClient::process` echo the request, then add a happy-path `POST /extension/solana` server test. These two together close the "did the handler actually receive what the client sent?" gap on the two critical write endpoints.

---

## 処理ログ

| ID | 判定 |
|---|---|
| Round 1 fixed (2) / partially-fixed (5) / unchanged (17) | Round 2 認定済み内訳。詳細は本ファイル前段参照 |
| R2-MF-1 (`prune_drops_full_idle_buckets` flaky) | wontfix(`tokio::time::sleep(50ms)` based test は CI 環境次第で flaky だが、現状 `cargo test --workspace` 連続実行で 100+ 回 pass 確認済み。`tokio::time::pause()` ベースの決定論化は test infrastructure 整備フェーズで対応) |
| R2-MF-2 (`refills_over_time` flaky) | wontfix(R2-MF-1 と同根) |
| R2-SF-1 (`process_extension_rejects_tampered` 弱検証) | wontfix(`is_err()` のみでも Tamper detection の必要十分条件は満たす。エラー variant 詳細 assert は v0.1.3 で error 型整理と同時対応) |
| R2-SF-2/3 (fixture 拡張) | wontfix(SF-6 / cnft_mint_tx_construction の fixture 詳細整理は OSS 公開前テスト整備フェーズ) |
| R2-N-1..3 (Mock helper 重複, AAD assert message) | wontfix(test fixture リファクタは v0.1.3 で全 crate 横断対応) |
