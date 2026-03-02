# Task 35: コード監査 — crates/tee

## 対象
`crates/tee/` — TEEサーバー (axum)

## ファイル
- `src/main.rs` — エントリポイント、ランタイム選択、axumルーター構築
- `src/config.rs` — TeeAppState, TeeState
- `src/error.rs` — TeeError enum + IntoResponse
- `src/runtime/mod.rs` — TeeRuntime trait
- `src/runtime/mock.rs` — MockRuntime (7テスト)
- `src/runtime/nitro.rs` — NitroRuntime + NsmOps抽象化 (8テスト)
- `src/endpoints/verify/` — /verify (handler, core, extension) (4テスト)
- `src/endpoints/sign/` — /sign (handler) (4テスト)
- `src/endpoints/create_tree.rs` — /create-tree (2テスト)
- `src/endpoints/test_helpers.rs` — テスト用モックサーバー
- `src/infra/gateway_auth.rs` — Gateway認証 (4テスト)
- `src/infra/proxy_client.rs` — プロキシクライアント
- `src/infra/security.rs` — DoS対策・リソース制限 (8テスト)
- `src/blockchain/solana_tx.rs` — Solanaトランザクション構築 (8テスト)
- `src/wasm_loader/` — WasmLoader trait + FileLoader + HttpLoader

## 監査で発見された問題

### バグ
1. **セマフォパーミットのリーク（direct版: 常時 / TCP版: エラーパス）**:
   - **direct版**: `permit.forget()` 後に `add_permits` が呼ばれず、PROXY_ADDR="direct" モードでリクエスト毎にパーミットがリーク。
   - **TCP版**: ループ内でチャンクタイムアウトやIOエラーが発生すると、`forget()` 済みパーミットが `add_permits()` に到達せずリーク。Slowloris攻撃でこのエラーパスを意図的に誘発し、セマフォを枯渇させることでReservation DoS防御を無効化できる。
   - → `SemaphoreGuard`（Drop実装）を導入し、成功/エラーの両パスで確実に解放。

### コード品質
2. **`b64()` ヘルパーが3箇所で重複定義**: `verify/mod.rs`, `sign/handler.rs`, `create_tree.rs` に同一関数。→ 共通モジュールに統合。
3. **`extension.rs` が `content_hash_from_manifest_signature` を誤用**: `wasm_hash` と `extension_input_hash` の計算にマニフェスト署名専用の関数を使用。意味的に `title_crypto::sha256()` を直接使うべき。→ 修正。
4. **MockRuntime テストがNitroRuntimeより少ない**: NitroRuntimeにある `test_tree_keypair_sign_verify` と `test_tee_type` がMockRuntimeにない。→ 2テスト追加。
5. **`detect_mime_type` / `format_content_hash` にテストがない**: /verifyエンドポイントの基盤ユーティリティが未テスト。→ 5テスト追加。
6. **`TeeError::IntoResponse` のStatusCodeマッピングにテストがない**: 10種類のエラーバリアントのHTTPステータスコード対応が未検証。→ 1テスト追加。

### 設計メモ（修正不要）
- TeeRuntime trait + MockRuntime/NitroRuntime のランタイム抽象化は堅実。
- NitroRuntime の NsmOps trait によるNSMデバイス抽象化でハードウェアなしテスト可能。
- 3エンドポイントの状態管理（Inactive→Active遷移、二重呼び出し防止）は正しい。
- セキュリティ防御3層（Zip Bomb + Reservation DoS + Slowloris）は仕様書 §6.4 準拠。
- Gateway認証のオプション化（開発環境スキップ）は設計通り。
- Solanaトランザクション構築は8テストで十分カバー。
- 全依存クレートが実際に使用されている（不要な依存なし）。
- TCP版のエラーパスリークはSlowlorisで攻撃可能（バグ#1として修正済み）。

## 完了基準
- [x] `proxy_get_secured_direct` のセマフォリーク修正
- [x] `b64()` 重複排除（共通モジュール化）
- [x] `extension.rs` の `content_hash_from_manifest_signature` → `sha256()` 修正
- [x] MockRuntime テスト追加（+2: tree_keypair, tee_type）
- [x] `detect_mime_type` / `format_content_hash` テスト追加（+5）
- [x] `TeeError::IntoResponse` テスト追加（+1）
- [x] `cargo test -p title-tee` パス（52テスト: 既存44 + 新規8）
- [x] `cargo check --workspace` パス（警告なし）

## 対処内容

### 1. セマフォリーク修正（SemaphoreGuard導入）
- `SemaphoreGuard` 構造体（`Drop` 実装）を導入。`acquire()` で `permit.forget()` + カウント蓄積し、Drop時に `add_permits()` で確実に解放。
- **TCP版**: ループ内の `permit.forget()` + 手動 `add_permits()` を `SemaphoreGuard` に置き換え。タイムアウトやIOエラーでの早期returnでもDropが走り、パーミットが解放される。
- **direct版**: 同じく `SemaphoreGuard` 経由で容量チェック + スコープ抜けで即解放。

### 2. `b64()` 重複排除
- `verify/mod.rs`, `sign/handler.rs`, `create_tree.rs` の3箇所の `b64()` を削除
- `endpoints/mod.rs` に `pub(crate) fn b64()` を1箇所に統合
- 各ファイルは `crate::endpoints::b64` / `super::b64` でインポート

### 3. `extension.rs` の `sha256()` 直接使用
- 旧: `title_crypto::content_hash_from_manifest_signature(&wasm_binary.bytes)` — マニフェスト署名専用関数をWASMバイナリに誤用
- 新: `title_crypto::sha256(&wasm_binary.bytes)` — 汎用SHA-256を直接使用（`ext_input_hash` も同様）

### 4. MockRuntime テスト追加（+2テスト）
- `test_tree_keypair_sign_verify` — Tree用キーペアの生成→署名→検証
- `test_tee_type` — TEE種別が "mock" であることを確認

### 5. ユーティリティ関数テスト追加（+5テスト）
- `test_detect_mime_type_jpeg` — JPEG マジックバイト
- `test_detect_mime_type_png` — PNG マジックバイト
- `test_detect_mime_type_webp` — WEBP マジックバイト
- `test_detect_mime_type_unknown` — 未知フォーマット + 空データ
- `test_format_content_hash` — 0x00..00 と 0xFF..FF の既知値

### 6. `TeeError::IntoResponse` テスト追加（+1テスト）
- `test_error_status_codes` — 全11バリアント（InvalidState + ServiceUnavailable が同じ503）のHTTPステータスコードマッピングを検証
