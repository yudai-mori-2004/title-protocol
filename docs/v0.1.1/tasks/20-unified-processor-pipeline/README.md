# Task 20: Unified Processor Pipeline

## Goal

Eliminate the core/extension code split in the verify pipeline. All processor_ids (including `core-c2pa`) go through a single code path. `core-c2pa` becomes a WASM module like every other processor.

## Background

Currently, `handler.rs` branches on `pid == "core-c2pa"`:
- `core.rs`: `verify_c2pa()` + `build_provenance_graph()` + TSA extraction + CorePayload + JCS sign
- `extension.rs`: WASM load + WASM execute + `verify_c2pa()` + ExtensionPayload + JCS sign

The two paths share the same skeleton (C2PA verification → content_hash → payload → JCS sign → SignedJson) but are implemented as separate functions with ~30 lines of duplicated signing code.

On the `/sign` side, core and extension are already handled uniformly — the only distinction is `protocol` field → Tree/Collection selection.

The spec says Core and Extension are a governance-level classification, not an architectural one. The code should reflect this.

## Design: Full Unification (Option B)

### Verify side

1. **Delete `core.rs`**. Remove `CORE_PROCESSOR_ID` branch in `handler.rs`.
2. **Single `process()` function** for all processor_ids (rename or merge into one module).
3. **`core-c2pa` becomes a WASM module** (`wasm/core-c2pa/`):
   - Calls host functions to extract provenance graph and TSA info
   - Returns JSON result like any other WASM module
4. **New host function ops** in `get_content_feature`:
   - `{"op": "c2pa_provenance_graph"}` → `{"nodes": [...], "links": [...]}`
   - `{"op": "c2pa_tsa_info"}` → `{"tsa_timestamp": N, "tsa_pubkey_hash": "...", "tsa_token_data": "..."}`
   - `{"op": "c2pa_content_type"}` → `{"content_type": "image/jpeg"}`
5. **CorePayload is abolished.** All processors use ExtensionPayload. Graph nodes/links and TSA fields are in the `result` (flattened).
6. **Protocol field unified.** `"Title-v1"` and `"Title-Extension-v1"` merge into a single protocol identifier.

### Sign side

7. **Tree routing** changes from protocol-field-based to `extension_id`-based:
   - `extension_id == "core-c2pa"` → Core Tree + Core Collection
   - All others → Extension Tree + Extension Collection

### On-chain

8. **`core-c2pa` registered as a WASM module** on GlobalConfig (same as image-pdq, cert-google, etc.).

### SDK / Indexer

9. **TypeScript types**: `CorePayload` removed. `SignedJson.payload` is always `ExtensionPayload` (or a unified `ProcessorPayload`).
10. **Indexer schema**: `core_cnfts` and `extension_cnfts` merge into a single `cnfts` table (or `core_cnfts` is removed and `extension_cnfts` renamed).

## Breaking Changes

- signed_json format for core-c2pa changes (CorePayload → ExtensionPayload with result)
- Protocol field name changes
- Existing on-chain cNFTs have the old format (migration or version tolerance needed)
- SDK type definitions change
- Indexer schema changes

## Files to Modify

| File | Change |
|------|--------|
| `crates/tee/src/endpoints/verify/core.rs` | Delete |
| `crates/tee/src/endpoints/verify/extension.rs` | Rename to `processor.rs`, handle all pids |
| `crates/tee/src/endpoints/verify/handler.rs` | Remove core/extension branch |
| `crates/tee/src/endpoints/verify/mod.rs` | Update exports |
| `crates/tee/src/endpoints/sign/handler.rs` | Change tree routing from protocol to extension_id |
| `crates/wasm-host/src/lib.rs` | Add host function ops for provenance graph, TSA, content_type |
| `crates/types/src/lib.rs` | Remove CorePayload, unify into single payload type |
| `wasm/core-c2pa/` | New WASM module (calls host functions) |
| `sdk/ts/src/types.ts` | Remove CorePayload, unify types |
| `indexer/src/db/schema.ts` | Merge tables |
| `docs/v0.1.1/SPECS_JA.md` | Update §3, §5.1 |

## Completion Criteria

- [ ] `handler.rs` has no `CORE_PROCESSOR_ID` branch
- [ ] `core.rs` does not exist
- [ ] `core-c2pa` is a WASM module in `wasm/core-c2pa/`
- [ ] All processor_ids go through a single code path
- [ ] `/sign` routes trees by `extension_id`, not `protocol`
- [ ] `cargo check --workspace && cargo test --workspace` passes
- [ ] Integration test: `core-c2pa` produces same content_hash and provenance graph as before
- [ ] `core-c2pa` WASM module registered on-chain (devnet)
