# Task 21: Cryptographic Abstraction Layer

## Goal

Replace all hardcoded cryptographic operations (Ed25519, X25519, AES-256-GCM) with a trait-based abstraction layer. Prepare the codebase for post-quantum cryptography (PQC) migration without changing any caller code.

## Background

The crypto layer had concrete functions (`ecdh_derive_shared_secret`, `aes_gcm_encrypt`, `ed25519_sign`, etc.) with algorithm-specific types (`Ed25519SigningKey`, `X25519StaticSecret`) leaking into the TEE endpoints. Algorithm identifiers were absent from wire formats and data structures. The TEE's encryption secret key was exposed as raw bytes (`encryption_secret_key() -> Vec<u8>`).

Research into RustCrypto (`signature`/`kem`/`aead` crates), HPKE (RFC 9180), TLS 1.3 (RFC 8446), and Go 1.26 `crypto` package established that the industry-standard abstraction is 3 pairs, not 2:

| Pair | Private Key | Public Key | Current Impl |
|------|------------|------------|-------------|
| Signing | `Signer` | `Verifier` | Ed25519 |
| KEM | `Decapsulator` | `Encapsulator` | X25519 ECDH |
| AEAD | `Aead::decrypt` | `Aead::encrypt` | AES-256-GCM |

## Design Decisions

1. **3-pair trait + composition functions** — Not 2-layer. Traits define primitives; `open_request`/`seal_for` compose KEM+KDF+AEAD without exposing internals to callers.
2. **No `SealedChannel` trait** — Concrete `ResponseSealer` struct. Only one implementation exists; `Box<dyn>` is unnecessary overhead.
3. **Direction-separated key derivation** — `request_key` and `response_key` derived via separate HKDF info strings (RFC 5869 §3.2), matching TLS 1.3 pattern. Eliminates nonce collision risk.
4. **AAD (Additional Authenticated Data)** — Mandatory parameter on AEAD trait. Prevents replay/swapping attacks by binding ciphertext to context.
5. **Domain tagging** — `domain_tagged("title-protocol-v1", msg)` prefix on all protocol signatures. Prevents cross-protocol attack when protocol_signer == solana_signer.
6. **`Signer::sign` returns `Result`** — Matches RustCrypto `signature::Signer::try_sign`. Supports HSM/remote signers.
7. **Wire format with `suite_id`** — `[suite_id(1B)][encap_key_len(2B)][encap_key][nonce][ciphertext]`. Self-describing for algorithm migration.
8. **`CryptoSuite` enum** — Bundles KEM+AEAD as a unit (like TLS cipher suites). Currently `0x01` = X25519 + AES-256-GCM.
9. **`OnceLock` for key storage** — Replaced `RwLock<Option<T>>` in TeeRuntime. Keys generated once, immutable after. Solves lifetime issue for `&dyn Signer` returns.
10. **`protocol_signer` / `solana_signer` separation** — Same key in Phase 1. Separate keys when PQC is introduced (Solana stays Ed25519).
11. **No backward compatibility** — New wire format only. Old `parse_encrypted_payload` removed. SDK updated separately.
12. **`shared_secret` not zeroized** — TEE threat model (Nitro Enclave memory is encrypted/isolated) makes this moot. Revisit in future hardening pass.
13. **`sign/handler.rs` uses runtime algorithm, not signed_json's claim** — Prevents downgrade attacks. The TEE verifies its own signatures; it knows the correct algorithm.

## Changes

### `crates/crypto/` — New modules

| File | Content |
|------|---------|
| `error.rs` | `CryptoError` (expanded: `UnsupportedSuite`, `NonceExhausted`, etc.) |
| `algorithm.rs` | `SigningAlgorithm`, `KemAlgorithm`, `AeadAlgorithm`, `CryptoSuite` + serde |
| `signing.rs` | `Signer` / `Verifier` traits |
| `kem.rs` | `Encapsulator` / `Decapsulator` traits (with contract docs) |
| `aead.rs` | `Aead` trait (with AAD parameter) |
| `sealed_channel.rs` | `OpenedRequest`, `ResponseSealer`, `open_request()`, `seal_for()` |
| `wire.rs` | `parse_wire()` / `build_wire()` (suite_id wire format) |
| `domain.rs` | `domain_tagged()` |
| `factory.rs` | `create_signer/verifier/decapsulator/encapsulator/aead` |
| `impls/ed25519.rs` | `Ed25519Signer`, `Ed25519Verifier` |
| `impls/x25519.rs` | `X25519Encapsulator`, `X25519Decapsulator` |
| `impls/aes256gcm.rs` | `Aes256GcmAead` |

Old API removed: `ecdh_derive_shared_secret`, `hkdf_derive_key`, `aes_gcm_encrypt`, `aes_gcm_decrypt`, `ed25519_sign`, `ed25519_verify`, `Ed25519SigningKey`, `Ed25519VerifyingKey`, `Ed25519Signature`, `SymmetricKey`.

### `crates/types/` — New fields

- `SignedJsonCore.tee_signature_algorithm: String` (required, no serde default)
- `TrustedTeeNode.signing_algorithm: String` (required, no serde default)
- `parse_encrypted_payload` / `ENCRYPTED_HEADER_SIZE` removed (dead code)

### `crates/tee/` — TeeRuntime overhaul

**TeeRuntime trait** (14 methods → 10):
```
generate_keypairs(), get_attestation(), tee_type()
protocol_signer() -> &dyn Signer
solana_signer() -> &dyn Signer
tree_signer() -> &dyn Signer
ext_tree_signer() -> &dyn Signer
decapsulator() -> &dyn Decapsulator
protocol_signing_algorithm(), kem_algorithm()
```

**MockRuntime / NitroRuntime**: `RwLock<Option<T>>` → `OnceLock<T>`.

**Endpoint changes:**
- `verify/handler.rs`: 5-step ECDH+HKDF+AES-GCM → `open_request()` + `channel.seal()`
- `verify/core.rs`, `extension.rs`: `domain_tagged` + `tee_signature_algorithm`
- `sign/handler.rs`: `create_verifier` + `domain_tagged` for verification, `solana_signer` for TX
- `create_tree.rs`: `solana_signer/tree_signer/ext_tree_signer`
- `register_node.rs`: `solana_signer/decapsulator`
- `config.rs`: `Option<Box<dyn Verifier>>`
- `gateway_auth.rs`: `Option<&dyn Verifier>`

### `crates/gateway/` — 2 test sites updated

`ed25519_verify` → `create_verifier().verify()`

### `docs/v0.1.1/SPECS_JA.md` — Spec updated

- §6.4.1 鍵管理: 3-pair architecture, algorithm identifiers, domain tagging
- §6.4.3 ハイブリッド暗号化: New wire format, direction-separated keys, AAD, `open_request`/`seal_for` flow
- §5.1 Step 2: Wire format updated
- §6.4.4 /verify: `open_request` + `protocol_signer` + `ResponseSealer`
- §6.4.5 /sign: `tee_signature_algorithm` + `domain_tagged` + `solana_signer`
- §6.4 防御モデル: "ECDH共通鍵" → "KEM導出対称鍵"

## Remaining Work (separate sessions)

- **TypeScript SDK**: Wire format + HKDF parameter update, `tee_signature_algorithm` / `signing_algorithm` fields
- **Solana program**: No change needed (PQC public keys will need off-chain/extended-PDA storage)
- **PQC Phase 2**: Separate `protocol_signer` / `solana_signer` keys, add ML-DSA/ML-KEM implementations

## Verification

```
cargo check --workspace  # 0 errors
cargo test --workspace   # 251 tests passed, 0 failed
```
