# Task 33: コード監査 — crates/crypto

## 対象
`crates/crypto/` — 暗号処理プリミティブ + Attestation Document検証

## ファイル
- `src/lib.rs` — ECDH, HKDF, AES-GCM, Ed25519, SHA-256
- `src/attestation/mod.rs` — TEE共通Attestation検証ディスパッチ
- `src/attestation/nitro.rs` — AWS Nitro Attestation Document検証（P-384証明書チェーン）

## 監査で発見された問題

### コード品質
1. **`lib.rs` にテストが一切ない**: 暗号プリミティブ（ECDH, HKDF, AES-GCM, Ed25519, SHA-256）が全てテスト未実装。下流の `crates/tee` テストで間接的に動作確認されているのみ。暗号クレートとして単体テストは必須。
2. **`serde` が不要な通常依存**: `crates/crypto/src/` 内のどのファイルにも `serde` の使用がない。`Cargo.toml` から削除可能。
3. **`sha256()` の不要なマニュアルコピー**: `Sha256::finalize()` → `GenericArray` を `.into()` で直接 `[u8; 32]` に変換可能。中間バッファ `hash` への `copy_from_slice` は不要。
4. **`attestation/mod.rs` の共通関数にテストがない**: `verify_measurements()`, `verify_public_key()` が `nitro.rs` テスト経由でしか検証されていない。Nitro固有の `From` 変換を通さない直接テストが必要。

### 設計メモ（修正不要）
- `attestation/nitro.rs`: 7テスト完備（ペイロードパース、COSE署名検証、PCR照合、公開鍵照合、X.509自己署名検証、AWSルート証明書デコード、共通型変換）。P-384自己署名証明書生成のテストヘルパーも堅実。
- feature flag `vendor-aws`: `nitro` モジュールの条件付きコンパイルは設計通り。
- `get_integer_field` の `i128 as u64` キャスト: Nitro timestamp は常に正の値なので実害はないが、防御的なコードとは言えない。ただし修正範囲が大きくないため据え置き。

## 完了基準
- [x] `lib.rs` に暗号プリミティブの単体テスト追加（ECDH+HKDF+AES-GCM roundtrip, Ed25519 sign/verify, SHA-256既知値, content_hash）
- [x] `serde` を `Cargo.toml` の依存から削除
- [x] `sha256()` を `.into()` で簡潔化
- [x] `attestation/mod.rs` に `verify_measurements` / `verify_public_key` の直接テスト追加
- [x] `cargo test -p title-crypto` パス（25テスト: lib 10 + attestation/mod 8 + nitro 7）
- [x] `cargo check --workspace` パス（警告なし）

## 対処内容

### 1. `serde` 依存削除
- `Cargo.toml` から `serde = { workspace = true }` を削除。ソースコード内で一切使用されていなかった。

### 2. `sha256()` 簡潔化
- 旧: `Sha256::new()` → `.update()` → `.finalize()` → 中間バッファに `copy_from_slice`
- 新: `Sha256::digest(data).into()` （1行）

### 3. `lib.rs` テスト追加（0 → 10テスト）
- `test_ecdh_hkdf_aes_gcm_roundtrip` — プロトコル §6.4 暗号化フロー全体
- `test_aes_gcm_wrong_key_fails` — 異なる鍵で復号失敗
- `test_aes_gcm_wrong_nonce_fails` — 異なるnonceで復号失敗
- `test_aes_gcm_tampered_ciphertext_fails` — 改竄された暗号文で復号失敗
- `test_ed25519_sign_verify_roundtrip` — 署名/検証往復
- `test_ed25519_wrong_message_fails` — メッセージ改竄で検証失敗
- `test_ed25519_wrong_key_fails` — 異なる鍵で検証失敗
- `test_sha256_known_value` — SHA-256("") の既知値照合
- `test_sha256_deterministic` — 同一入力で同一出力
- `test_content_hash_is_sha256_of_signature` — content_hash = sha256(signature)

### 4. `attestation/mod.rs` テスト追加（0 → 8テスト）
- `test_verify_measurements_match` — 一致するPCR値
- `test_verify_measurements_mismatch` — 不一致のPCR値
- `test_verify_measurements_missing_key` — 存在しないキー
- `test_verify_measurements_empty_expected` — 空の期待値 → 常にtrue
- `test_verify_public_key_match` — 一致する公開鍵
- `test_verify_public_key_mismatch` — 不一致の公開鍵
- `test_verify_public_key_none` — public_key=None
- `test_verify_attestation_unsupported_tee_type` — 未対応TEE種別
