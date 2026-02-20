# CLAUDE.md

## プロジェクト概要

Title Protocol: デジタルコンテンツの帰属をブロックチェーン（Solana）に記録するプロトコル。
C2PA（来歴証明）× TEE（信頼された実行環境）× cNFT（圧縮NFT）を組み合わせ、コンテンツの「誰のものか」をトラストレスに解決する。

- 技術仕様書: `docs/SPECS_JA.md`（ver.9）
- 実装カバレッジ: `docs/COVERAGE.md`
- タスク定義: `docs/tasks/` 配下

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
- 未実装部分は `todo!()` で明示（コンパイルは通る状態を維持）
- 仕様書のJSON構造とRust構造体のフィールド名は一致させる（snake_case）
- WASMモジュールは `#![no_std]` + `dlmalloc` グローバルアロケータ + `core::arch::wasm32::unreachable()` パニックハンドラ
- テストは各クレート内に `#[cfg(test)] mod tests` で書く
- `prototype/` と `docs/` 配下の既存ファイルは編集しない（参照のみ可）

## アーキテクチャ

```
Client (SDK) → Gateway → Temporary Storage → TEE → Solana
                                              ↓
                                         Off-chain Storage (Arweave)
```

### Rustクレート（workspace members）

| クレート | 役割 | 依存方向 |
|---------|------|---------|
| `crates/types` | 全コンポーネントが依存する型定義 | 他から依存される |
| `crates/crypto` | 暗号プリミティブ（**実装済み**） | types に依存 |
| `crates/core` | C2PA検証 + 来歴グラフ構築 | types, crypto に依存 |
| `crates/wasm-host` | wasmtime直接使用のWASM実行環境 | types に依存 |
| `crates/tee` | TEEサーバー本体（axum） | types, crypto, core, wasm-host に依存 |
| `crates/gateway` | Gateway HTTPサーバー（axum） | types に依存 |
| `crates/proxy` | vsock HTTPプロキシ（TEE↔外部通信） | 独立 |

### WASMモジュール（workspace外、個別ビルド）

| モジュール | 出力 |
|-----------|------|
| `wasm/phash-v1` | 知覚ハッシュ |
| `wasm/hardware-google` | ハードウェア撮影証明 |
| `wasm/c2pa-training-v1` | AI学習許可フラグ |
| `wasm/c2pa-license-v1` | ライセンス情報 |

### TypeScript

| パッケージ | 役割 |
|-----------|------|
| `sdk/ts` | クライアントSDK（register, resolve, discover） |
| `indexer` | cNFTインデクサ（webhook + poller） |

### Solanaプログラム

| プログラム | 役割 |
|-----------|------|
| `programs/title-config` | Global Config PDA管理（Anchor、**実装済み**） |

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

各タスクは `docs/tasks/` に定義ファイルがある。セッション開始時に指定されたタスクファイルを読み、そこに記載された「読むべきファイル」「仕様書セクション」「要件」「完了条件」に従って作業する。

**1タスク = 1セッション**。コンテキストの溢れを防ぐため、1セッションで1タスクに集中する。

作業完了後は必ず:
1. `docs/COVERAGE.md` の該当箇所を「スタブ」→「実装済み」に更新
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
```

| # | タスクファイル | 内容 | 前提 |
|---|--------------|------|------|
| 01 | `docs/tasks/01-mock-runtime.md` | MockRuntime（鍵生成・署名・Attestation） | なし |
| 02 | `docs/tasks/02-proxy.md` | vsock HTTPプロキシ非同期化 | なし |
| 03 | `docs/tasks/03-c2pa-core.md` | C2PA検証 + 来歴グラフ構築 | なし |
| 04 | `docs/tasks/04-tee-verify.md` | TEE /verifyエンドポイント + proxy_client | 01, 02, 03 |
| 05 | `docs/tasks/05-tee-sign.md` | TEE /sign + /create-tree + Bubblegum | 04 |
| 06 | `docs/tasks/06-gateway.md` | Gateway全ハンドラ + Gateway認証 | 05 |
| 07 | `docs/tasks/07-wasm-host-and-modules.md` | WASMホスト + 4モジュール実装 | 04 |
| 08 | `docs/tasks/08-ts-sdk.md` | TypeScript SDK全関数 | 06 |
| 09 | `docs/tasks/09-indexer.md` | インデクサ（Webhook + Poller + DB） | 05 |
| 10 | `docs/tasks/10-security-hardening.md` | DoS対策・リソース制限・防御強化 | 06, 07 |
| 11 | `docs/tasks/11-nitro-runtime.md` | NitroRuntime + Enclave本番ビルド | 01, 02 |
| 12 | `docs/tasks/12-e2e-local-dev.md` | E2Eテスト + ローカル開発環境整備 | 01〜09 |

タスク01, 02, 03は前提なし。並行で着手可能。
