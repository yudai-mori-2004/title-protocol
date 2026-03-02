# タスク13: コードベース整理 — オープンソース公開品質への引き上げ

## 背景

タスク01〜12が全て完了し、全テストが通る状態になった。
本タスクは、コードの新規機能追加ではなく、オープンソースプロトコルとしての品質・可読性・構造の最終整備を行う。

Title Protocolは**中立的な公共インフラ**であり、各ノード運営者がフォークして自由にノードを構築する前提のコードベースである。
そのため、以下の観点を重視する:

- **初見の開発者が迷わない構造**（ディレクトリ構成、モジュール分割、命名）
- **仕様書との対応が明確**（docコメントのセクション参照、CLAUDE.mdの正確性）
- **コンパイラ警告ゼロ**（dead code、deprecated API）
- **COVERAGE.mdとCLAUDE.mdが現状を正確に反映している**（AI開発基盤として信頼できる状態）

## 前提タスク

- タスク01〜12が全て完了していること

## 読むべきファイル

1. `docs/SPECS_JA.md` — 仕様書全体（セクション番号の確認）
2. `docs/COVERAGE.md` — 現在のカバレッジ記述
3. `CLAUDE.md` — AI開発ガイドライン
4. `README.md` — 公開ドキュメント
5. `cargo check --workspace 2>&1` の警告出力
6. 各クレートの `src/` 配下（docコメントの網羅性チェック）

## 作業内容

### 1. コンパイラ警告の解消

現在 `cargo check --workspace` で以下の4警告が出ている。全て解消する:

#### 1.1 `title-crypto`: deprecated `from_slice` (2箇所)
- `crates/crypto/src/lib.rs` の `Nonce::from_slice()` が `generic-array` 0.x の非推奨API
- 原因: `aes-gcm` が内部で `generic-array` を使用
- 対応: `GenericArray::clone_from_slice()` への置き換え、または非推奨警告を明示的に suppress（上流の問題のため `#[allow(deprecated)]` + コメントで理由記載）

#### 1.2 `title-tee`: `MockNsm` never constructed
- `crates/tee/src/runtime/nitro.rs:134` — テスト用構造体が `#[cfg(test)]` の外にある
- 対応: `#[cfg(test)]` ゲート内に移動

#### 1.3 `title-tee`: `proxy_post` never used
- `crates/tee/src/proxy_client.rs:154` — 宣言されているが使用箇所なし
- 対応: 現在使われていなければ削除。将来必要になったら再実装する

### 2. Gateway の構造分割

`crates/gateway/src/main.rs` が **949行** と1ファイルに詰め込まれすぎている。
TEEクレート（`crates/tee/`）は `endpoints/`, `runtime/`, `security.rs` 等に適切に分割されているのと対照的。

以下のモジュール構成に分割する:

```
crates/gateway/src/
├── main.rs              — エントリポイント（起動ロジック + ルーティング定義のみ）
├── config.rs            — 環境変数読み込み、GatewayState構築
├── storage.rs           — TempStorageトレイト + S3TempStorage実装
├── auth.rs              — Gateway認証（Ed25519署名の付与）
├── error.rs             — GatewayError定義
└── endpoints/
    ├── mod.rs           — pub use
    ├── upload_url.rs    — POST /upload-url
    ├── verify.rs        — POST /verify
    ├── sign.rs          — POST /sign
    ├── sign_and_mint.rs — POST /sign-and-mint
    └── node_info.rs     — GET /.well-known/title-node-info
```

分割の原則:
- **TEEクレートと対称的な構造にする**（endpoints/ サブディレクトリの採用）
- 1エンドポイント = 1ファイル
- main.rsは起動ロジックとルーティングテーブルだけ（50行以下が理想）

### 3. Proxy の構造分割

`crates/proxy/src/main.rs` が **447行** で、プロトコル処理・テストが全て1ファイル。

以下のモジュール構成に分割する:

```
crates/proxy/src/
├── main.rs              — エントリポイント（起動ロジックのみ）
├── protocol.rs          — Length-prefixed protocol のエンコード/デコード
├── handler.rs           — HTTP転送ロジック
└── listener.rs          — vsock/TCPリスナー（cfg分岐）
```

### 4. COVERAGE.md の整合性修正

現在のCOVERAGE.mdには以下の不整合がある:

#### 4.1 §7.1 のステータスが古い
- L256: `| Fuel/Memory制限 | ... | **スタブ** —` → 実装済み（WasmRunnerで適用済み）
- L258: `| Core→Extension処理順序の保証 | — | **未着手**` → verify.rsのprocess_core→process_extensionで実装済み
- L259-263: ホスト関数4種が「未着手」になっているが、wasm-hostで全て実装済み

#### 4.2 §5.1 のステータスが「型のみ」のまま
- L133-141: 全Step の状態が「型のみ」だが、TEEエンドポイント・SDK・Gatewayが実装済みなので、使用されているフローとしては「実装済み」

#### 4.3 優先度マップセクション（L323-356）が陳腐化
- 全項目が実装済みなのに優先度リストが残っている
- 削除するか、「全て完了」の旨を明記する

### 5. CLAUDE.md の更新

#### 5.1 実装ステータスの更新
- 「スタブ」の記述を全て現状に合わせる
- アーキテクチャ表にファイル行数/モジュール分割の構造を反映

#### 5.2 仕様書セクション参照の追加
- クレート一覧テーブルに仕様書セクション番号を追加:

```markdown
| クレート | 役割 | 仕様書 |
|---------|------|--------|
| `crates/types` | 全コンポーネントが依存する型定義 | §5 |
| `crates/crypto` | 暗号プリミティブ | §1.1 Phase 1 Step 2, §6.4 |
| `crates/core` | C2PA検証 + 来歴グラフ構築 | §2.1, §2.2 |
| `crates/wasm-host` | wasmtime直接使用のWASM実行環境 | §7.1 |
| `crates/tee` | TEEサーバー本体（axum） | §6.4, §1.1 |
| `crates/gateway` | Gateway HTTPサーバー（axum） | §6.2 |
| `crates/proxy` | vsock HTTPプロキシ | §6.4 |
```

#### 5.3 タスク一覧の完了ステータス追記
- 全12タスクが完了済みである旨を記載
- タスク13（本タスク）を追加

### 6. .env.example の作成

環境変数が各バイナリに散在しており、ノード運営者が何を設定すべきか不明確。
コンポーネント別に全環境変数を列挙した `.env.example` を作成する:

```env
# ===========================================
# Title Protocol 環境変数
# ===========================================

# --- 共通 ---
SOLANA_RPC_URL=https://api.devnet.solana.com

# --- Gateway (crates/gateway) ---
GATEWAY_SIGNING_KEY=            # Ed25519秘密鍵（Base58）
TEE_ENDPOINT=http://localhost:4000
MINIO_ENDPOINT=http://localhost:9000
MINIO_PUBLIC_ENDPOINT=http://localhost:9000
MINIO_ACCESS_KEY=minioadmin
MINIO_SECRET_KEY=minioadmin

# --- TEE (crates/tee) ---
TEE_RUNTIME=mock                # "mock" | "nitro"
MOCK_MODE=true                  # true = MockRuntime使用
PROXY_ADDR=direct               # "direct" | "vsock:8000" | "tcp:127.0.0.1:8000"
COLLECTION_MINT=                # cNFTコレクションのMintアドレス
GATEWAY_PUBKEY=                 # Gateway認証用Ed25519公開鍵（Base58、省略可）
TRUSTED_EXTENSIONS=phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1
ARWEAVE_GATEWAY=http://localhost:1984
WASM_DIR=/wasm-modules

# --- Proxy (crates/proxy) ---
# Linux: vsock port 8000 (自動)
# macOS: TCP 127.0.0.1:8000 (自動)

# --- Indexer (indexer/) ---
DATABASE_URL=postgres://title:title_dev@localhost:5432/title_indexer
DAS_ENDPOINTS=https://api.devnet.solana.com
COLLECTION_MINTS=               # 監視対象コレクションMint（カンマ区切り）
WEBHOOK_SECRET=                 # Webhook認証シークレット
```

### 7. docコメントの仕様書参照の補強

CLAUDE.mdの規約:「全てのRust公開関数にdocコメント（日本語）。仕様書の該当セクション番号を含める」

以下のファイルでdocコメントの仕様書参照が不足している:

- `crates/core/src/lib.rs` — `verify_c2pa()`, `extract_content_hash()`, `build_provenance_graph()` に `§2.1`, `§2.2` 参照を追加
- `crates/tee/src/endpoints/verify.rs` — `handle_verify()`, `process_core()`, `process_extension()` に `§1.1 Phase 1`, `§3.1` 参照を追加
- `crates/tee/src/endpoints/sign.rs` — `handle_sign()` に `§1.1 Phase 2` 参照を追加
- `crates/tee/src/endpoints/create_tree.rs` — `§6.5` 参照を追加
- `crates/gateway/src/main.rs` — 各ハンドラに `§6.2` 参照を追加
- `crates/tee/src/security.rs` — `§6.4` セキュリティ関連セクション参照を追加
- `crates/tee/src/solana_tx.rs` — `§6.5` Bubblegum参照を追加
- `crates/wasm-host/src/lib.rs` — `§7.1` 参照を追加

### 8. 空ディレクトリの整理

- `tests/integration/.gitkeep` — 使われていない。E2Eテストは `tests/e2e/` にある
- `tests/fixtures/.gitkeep` — 使われていない。フィクスチャは `tests/e2e/fixtures/` と `crates/core/tests/fixtures/` にある

対応: 空ディレクトリを削除する（将来必要なら再作成すればよい）

### 9. node_modulesのgitignore確認

`tests/e2e/node_modules/` がgitにトラッキングされていないか確認。
`.gitignore` に `node_modules/` が含まれていれば問題なし。含まれていなければ追加。

### 10. README.md の更新

現在のREADMEは概念説明のみで、コードの構造やビルド方法が記載されていない。
オープンソースリポジトリとして最低限必要な以下のセクションを追加:

- **Quick Start**: `cargo check --workspace && cargo test --workspace`
- **Repository Structure**: ディレクトリの役割一覧（簡潔に）
- **Running a Node**: `docker-compose up` + `scripts/setup-local.sh` の手順
- **For Developers**: CLAUDE.md、タスクファイル、仕様書への導線
- **Configuration**: `.env.example` への参照

日本語仕様書のプロジェクトだが、READMEは英語で書く（オープンソースの慣例）。
既存の概念説明部分はそのまま維持する。

## 完了条件

- [ ] `cargo check --workspace` で **警告ゼロ**
- [ ] `cargo test --workspace` が全て通る
- [ ] Gateway が TEE と対称的なモジュール構成に分割されている
- [ ] Proxy が適切にモジュール分割されている
- [ ] `docs/COVERAGE.md` が現在の実装状態を正確に反映している
- [ ] `CLAUDE.md` が現在の実装状態を正確に反映している（仕様書セクション参照付き）
- [ ] `.env.example` が全環境変数を網羅している
- [ ] 全公開関数のdocコメントに仕様書セクション番号が含まれている
- [ ] 空ディレクトリが整理されている
- [ ] `README.md` にビルド方法・構造説明・ノード起動方法が記載されている
- [ ] `cargo check --workspace && cargo test --workspace` が通る（最終確認）
