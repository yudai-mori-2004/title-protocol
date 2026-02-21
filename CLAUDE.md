# CLAUDE.md

## プロジェクト概要

Title Protocol: デジタルコンテンツの帰属をブロックチェーン（Solana）に記録するプロトコル。
C2PA（来歴証明）× TEE（信頼された実行環境）× cNFT（圧縮NFT）を組み合わせ、コンテンツの「誰のものか」をトラストレスに解決する。

- ドキュメント管理: `docs/README.md`（バージョン単位で SPECS→COVERAGE→tasks を管理）
- 現行バージョン: `docs/v1/`（2026-02-21、初期実装、全タスク完了）
  - 技術仕様書: `docs/v1/SPECS_JA.md`（ver.9）
  - 実装カバレッジ: `docs/v1/COVERAGE.md`（仕様→実装の橋渡し、累積的）
  - タスク定義: `docs/v1/tasks/NN-name/`（各タスクはディレクトリ、メモを併置可能）
- 環境変数一覧: `.env.example`

## ビルド手順

```bash
# Rust workspace（7クレート）
cargo check --workspace
cargo test --workspace

# WASMモジュール（4モジュール、workspaceから除外されているため個別ビルド）
cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release
cd wasm/hardware-google && cargo build --target wasm32-unknown-unknown --release
cd wasm/c2pa-training-v1 && cargo build --target wasm32-unknown-unknown --release
cd wasm/c2pa-license-v1 && cargo build --target wasm32-unknown-unknown --release

# TypeScript SDK
cd sdk/ts && npm run build

# TypeScript Indexer
cd indexer && npm run build

# Anchorプログラム（要anchor CLI）
cd programs/title-config && anchor build
```

## コーディング規則

- 全てのRust公開関数にdocコメント（日本語）。仕様書の該当セクション番号を含める（例: `/// 仕様書 §5.1 Step 4`）
- エラー型は `thiserror` で定義し、クレートごとに専用のError enumを持つ
- 仕様書のJSON構造とRust構造体のフィールド名は一致させる（snake_case）
- WASMモジュールは `#![no_std]` + `dlmalloc` グローバルアロケータ + `core::arch::wasm32::unreachable()` パニックハンドラ
- テストは各クレート内に `#[cfg(test)] mod tests` で書く
- `prototype/` は編集しない（参照のみ可）
- 完了済みバージョン（`docs/v1/` 等）は原則編集しない。現行バージョンのみ編集対象

## アーキテクチャ

```
Client (SDK) → Gateway → Temporary Storage → TEE → Solana
                                              ↓
                                         Off-chain Storage (Arweave)
```

### Rustクレート（workspace members）

| クレート | 役割 | 仕様書 | 状態 |
|---------|------|--------|------|
| `crates/types` | 全コンポーネントが依存する型定義 | §5 | **実装済み** |
| `crates/crypto` | 暗号プリミティブ | §1.1 Phase 1 Step 2, §6.4 | **実装済み** |
| `crates/core` | C2PA検証 + 来歴グラフ構築 | §2.1, §2.2 | **実装済み** |
| `crates/wasm-host` | wasmtime直接使用のWASM実行環境 | §7.1 | **実装済み** |
| `crates/tee` | TEEサーバー本体（axum） | §6.4, §1.1 | **実装済み** |
| `crates/gateway` | Gateway HTTPサーバー（axum） | §6.2 | **実装済み** |
| `crates/proxy` | vsock HTTPプロキシ（TEE↔外部通信） | §6.4 | **実装済み** |

### モジュール構成の詳細

**Gateway** (`crates/gateway/src/`):
```
main.rs          — エントリポイント + ルーティング定義
config.rs        — GatewayState（共有状態）
storage.rs       — TempStorageトレイト + S3TempStorage実装
auth.rs          — Gateway認証（Ed25519署名の付与・TEE中継）
error.rs         — GatewayError定義
endpoints/       — 各エンドポイントハンドラ（1ファイル = 1エンドポイント）
```

**TEE** (`crates/tee/src/`):
```
main.rs          — エントリポイント + ルーティング
endpoints/       — verify.rs, sign.rs, create_tree.rs
runtime/         — TeeRuntimeトレイト, mock.rs, nitro.rs
security.rs      — DoS対策（セマフォ、タイムアウト）
proxy_client.rs  — vsock/HTTP経由の外部通信
gateway_auth.rs  — Gateway認証検証
solana_tx.rs     — Bubblegumトランザクション構築
```

**Proxy** (`crates/proxy/src/`):
```
main.rs          — エントリポイント（vsock/TCPリスナー）
protocol.rs      — Length-prefixedプロトコルのエンコード/デコード
handler.rs       — HTTP転送ロジック
```

### WASMモジュール（workspace外、個別ビルド）

| モジュール | 出力 | 仕様書 |
|-----------|------|--------|
| `wasm/phash-v1` | 知覚ハッシュ | §7.4 |
| `wasm/hardware-google` | ハードウェア撮影証明 | §7.4 |
| `wasm/c2pa-training-v1` | AI学習許可フラグ | §7.4 |
| `wasm/c2pa-license-v1` | ライセンス情報 | §7.4 |

### TypeScript

| パッケージ | 役割 | 仕様書 |
|-----------|------|--------|
| `sdk/ts` | クライアントSDK（register, resolve, discover） | §6.7 |
| `indexer` | cNFTインデクサ（webhook + poller） | §6.6 |

### Solanaプログラム

| プログラム | 役割 | 仕様書 |
|-----------|------|--------|
| `programs/title-config` | Global Config PDA管理（Anchor） | §8 |

### プロトタイプ（参照用、編集禁止）

| ディレクトリ | 内容 |
|------------|------|
| `prototype/enclave-c2pa` | Nitro Enclaveでの動作実証済みコード |
| `prototype/enclave-c2pa/proxy/` | vsock HTTPプロキシの同期版実装 |

## 重要な設計判断

- **Extismは使わない**。wasmtimeを直接使用する（§7.1）
- **c2paクレートはv0.47**（v0.44は内部依存の衝突で不可）
- **vsockはLinux条件付きコンパイル**: `#[cfg(target_os = "linux")]`。macOSではTCPフォールバック
- **TEEのランタイムはtrait抽象化**: `trait TeeRuntime` → `MockRuntime`（ローカル）/ `NitroRuntime`（本番）
- **TEEはステートレス**: リクエスト間で状態を持たない。鍵はメモリ上のみ、再起動で消滅
- **vsockプロキシプロトコル**: length-prefixed format（`prototype/enclave-c2pa/proxy/` と同一）
  - TEE→Proxy: `[4B: method_len][method][4B: url_len][url][4B: body_len][body]`
  - Proxy→TEE: `[4B: status_code][4B: body_len][body]`

## タスクの進め方

各タスクは `docs/vN/tasks/NN-name/README.md` に定義がある。セッション開始時に指定されたタスクの `README.md` を読み、そこに記載された「読むべきファイル」「仕様書セクション」「要件」「完了条件」に従って作業する。作業中に発見した知見・罠・メモは同じタスクディレクトリに `.md` ファイルとして残す。

**1タスク = 1セッション**。コンテキストの溢れを防ぐため、1セッションで1タスクに集中する。

作業完了後は必ず:
1. 該当バージョンの `docs/vN/COVERAGE.md` を更新
2. `cargo check --workspace && cargo test --workspace` が通ることを確認

## タスク一覧と依存関係

```
01 MockRuntime ─────┐
                    ├─→ 04 TEE /verify ─→ 05 TEE /sign ─→ 06 Gateway ─→ 08 TS SDK
02 Proxy ───────────┤                                        │
                    │                                        └─→ 10 Security
03 C2PA Core ───────┘
                         04 TEE /verify ─→ 07 WASM Host+Modules ─→ 10 Security
                         05 TEE /sign ──→ 09 Indexer
                         01 MockRuntime ─→ 11 NitroRuntime
                         01〜09 全完了 ──→ 12 E2E + ローカル環境
                         01〜12 全完了 ──→ 13 コードベース整理
                         01〜16 全完了 ──→ 17 Devnetデプロイ基盤
                         01〜17 全完了 ──→ 18 ベンダー名除去+SDK再設計
```

| # | タスクファイル | 内容 | 状態 |
|---|--------------|------|------|
| 01 | `docs/v1/tasks/01-mock-runtime/` | MockRuntime（鍵生成・署名・Attestation） | **完了** |
| 02 | `docs/v1/tasks/02-proxy/` | vsock HTTPプロキシ非同期化 | **完了** |
| 03 | `docs/v1/tasks/03-c2pa-core/` | C2PA検証 + 来歴グラフ構築 | **完了** |
| 04 | `docs/v1/tasks/04-tee-verify/` | TEE /verifyエンドポイント + proxy_client | **完了** |
| 05 | `docs/v1/tasks/05-tee-sign/` | TEE /sign + /create-tree + Bubblegum | **完了** |
| 06 | `docs/v1/tasks/06-gateway/` | Gateway全ハンドラ + Gateway認証 | **完了** |
| 07 | `docs/v1/tasks/07-wasm-host-and-modules/` | WASMホスト + 4モジュール実装 | **完了** |
| 08 | `docs/v1/tasks/08-ts-sdk/` | TypeScript SDK全関数 | **完了** |
| 09 | `docs/v1/tasks/09-indexer/` | インデクサ（Webhook + Poller + DB） | **完了** |
| 10 | `docs/v1/tasks/10-security-hardening/` | DoS対策・リソース制限・防御強化 | **完了** |
| 11 | `docs/v1/tasks/11-nitro-runtime/` | NitroRuntime + Enclave本番ビルド | **完了** |
| 12 | `docs/v1/tasks/12-e2e-local-dev/` | E2Eテスト + ローカル開発環境整備 | **完了** |
| 13 | `docs/v1/tasks/13-codebase-cleanup/` | コードベース整理（警告解消・モジュール分割・ドキュメント整備） | **完了** |
| 14 | `docs/v1/tasks/14-hmac-content/` | hmac_content WASMホスト関数 | **完了** |
| 15 | `docs/v1/tasks/15-tsa-resolution/` | TSAタイムスタンプ重複解決 | **完了** |
| 16 | `docs/v1/tasks/16-collection-delegate/` | Collection Authority Delegate | **完了** |
| 17 | `docs/v1/tasks/17-devnet-deploy/` | Devnetデプロイ基盤（Terraform + スクリプト） | **完了** |
| 18 | `docs/v1/tasks/18-vendor-neutrality/` | ベンダー名除去 + SDK粒度再設計 | **完了** |
