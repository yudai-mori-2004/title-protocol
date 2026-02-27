# Task 36: コード監査 — crates/gateway

## 対象
`crates/gateway/` — Gateway HTTPサーバー (axum)

## ファイル
- `src/main.rs` — エントリポイント、axumルーター構築、テスト6件
- `src/config.rs` — GatewayState（共有状態）
- `src/error.rs` — GatewayError enum + IntoResponse
- `src/auth.rs` — Gateway認証（b64, build_gateway_auth_wrapper, relay_to_tee）
- `src/storage/mod.rs` — TempStorage trait, PresignedUrls
- `src/storage/s3.rs` — S3TempStorage（vendor-aws feature）
- `src/endpoints/mod.rs` — ハンドラ再エクスポート
- `src/endpoints/verify.rs` — POST /verify
- `src/endpoints/sign.rs` — POST /sign
- `src/endpoints/sign_and_mint.rs` — POST /sign-and-mint
- `src/endpoints/node_info.rs` — GET /.well-known/title-node-info
- `src/endpoints/upload_url.rs` — POST /upload-url

## 監査で発見された問題

### バグ
1. **`sign_and_mint.rs` の `tx.signatures[sig_index]` に境界チェックなし**:
   `account_keys` 全体を `position()` で検索しているため、`gateway_pubkey` が非署名者位置（index >= num_required_signatures）にある場合、`tx.signatures[sig_index]` がVecの境界外アクセスでpanic。
   → `sig_index < tx.signatures.len()` のチェックを追加し、GatewayError で返す。

### コード品質
2. **`title-crypto` が通常依存に含まれているがテストでのみ使用**:
   `title_crypto::ed25519_verify` はテストコード（`main.rs` L253, L284）でのみ参照。本番コードでは `ed25519_dalek` を直接使用。
   → `[dev-dependencies]` に移動。

### テストギャップ
3. **`GatewayError::IntoResponse` のステータスコードマッピングにテストがない**:
   5バリアントのHTTPステータスコード対応が未検証。TeeErrorと同パターン。
   → テスト追加（+1）。
4. **`/sign-and-mint` エンドポイントにテストがない**:
   最も複雑なエンドポイント（TEE中継 + Solana署名 + RPCブロードキャスト）がテスト0件。
   設定未設定時のエラーパス（solana_rpc_url=None, solana_keypair=None）はテスト可能。
   → 設定未設定エラーパスのテスト追加（+2）。
   ※ 正常系（mock Solana RPC）はコスト対効果が低いためスキップ。TEE中継は/signで検証済み。
5. **TEE中継エラー伝播のテストがない**:
   TEEが非200を返した場合にGatewayが502 BAD_GATEWAYで返すことが未検証。
   → テスト追加（+1）。

### 設計メモ（修正不要）
- TempStorage traitによるストレージ抽象化は堅実。
- S3TempStorageのpublic/internal bucket分離（Docker内外ホスト名対応）は正しい設計。
- vendor-aws featureによるベンダー分離は仕様通り。
- Gateway認証（Ed25519署名）のラウンドトリップテストが既にある。
- 5エンドポイントの構造が明確に分離されている。
- 全21依存クレートが実際に使用されている（title-cryptoのみテスト限定）。

## 完了基準
- [x] `sign_and_mint.rs` の署名者インデックス境界チェック追加
- [x] `title-crypto` を `[dev-dependencies]` に移動
- [x] `GatewayError::IntoResponse` テスト追加（+1）
- [x] `/sign-and-mint` 設定未設定エラーパステスト追加（+2）
- [x] TEE中継エラー伝播テスト追加（+1）
- [x] `cargo test -p title-gateway` パス（10テスト: 既存6 + 新規4）
- [x] `cargo check --workspace` パス（警告なし）

## 対処内容

### 1. 署名者インデックス境界チェック追加
- 旧: `tx.message.account_keys.iter().position(|k| *k == gateway_pubkey)` — 全account_keysを検索
- 新: `tx.message.account_keys.iter().take(num_signers).position(...)` — 署名者（先頭num_required_signatures個）のみ検索
- これにより、非署名者位置に一致した場合の`tx.signatures[sig_index]`境界外アクセスpanicを防止。

### 2. `title-crypto` → `[dev-dependencies]`
- `title_crypto::ed25519_verify` はテストコード内でのみ使用。本番コードは `ed25519_dalek` を直接使用。
- `[dependencies]` から削除し `[dev-dependencies]` に移動。

### 3. `GatewayError::IntoResponse` テスト追加（+1テスト）
- `test_error_status_codes` — 全5バリアント（TeeRelay→502, Storage→500, Solana→502, Internal→500, BadRequest→400）のHTTPステータスコードマッピングを検証。

### 4. `/sign-and-mint` エラーパステスト追加（+2テスト）
- `test_sign_and_mint_no_rpc_url` — SOLANA_RPC_URL未設定時にエラーが返ることを確認。
- `test_sign_and_mint_no_keypair` — GATEWAY_SOLANA_KEYPAIR未設定（RPC URLは設定済み）時にエラーが返ることを確認。

### 5. TEE中継エラー伝播テスト追加（+1テスト）
- `test_verify_relay_tee_error` — TEEが500を返した場合にGatewayが502 BAD_GATEWAYで返すことを確認。
