# Task 15: TSA鍵管理 — CLI + ドキュメント整備

## 背景

TeeNode と WasmModule にはオンチェーン命令・CLIコマンド・ドキュメントが揃っているのに対し、TSA鍵は対称性が欠けている。

### 現状の対称性

| 観点 | TeeNode | WasmModule | TSA Key |
|------|---------|------------|---------|
| オンチェーン命令 | `register_tee_node` / `remove_tee_node` | `register_wasm_module` / `remove_wasm_module` | `add_tsa_key` / `remove_tsa_key` |
| CLI コマンド | `register-node` / `remove-node` | `register-wasm` / `remove-wasm` / `add-wasm-version` | **なし** |
| ドキュメント (programs/title-config/README.md) | 記載あり | 記載あり | **記載なし** |
| ドキュメント (docs/reference.md) | 記載あり | 記載あり | **記載なし** |
| 個別PDA | TeeNodeAccount | WasmModuleAccount | なし（設計上不要） |

オンチェーン命令（`add_tsa_key` / `remove_tsa_key`）は実装済み。CLIとドキュメントだけが欠けている。

## 作業内容

### 1. CLI コマンド追加

- `title-cli add-tsa-key --key <Base58 pubkey hash>` — `add_tsa_key` 命令を呼ぶ
- `title-cli remove-tsa-key --key <Base58 pubkey hash>` — `remove_tsa_key` 命令を呼ぶ
- `anchor.rs` に `build_add_tsa_key_ix` / `build_remove_tsa_key_ix` を追加
- `main.rs` にサブコマンド定義を追加
- `commands/` に `add_tsa_key.rs` / `remove_tsa_key.rs` を追加

### 2. ドキュメント更新

- `programs/title-config/README.md` — 命令一覧に `add_tsa_key` / `remove_tsa_key` を追加
- `docs/reference.md` — TSA鍵管理手順を追加

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `programs/title-config/src/lib.rs` L513-534 | `add_tsa_key` / `remove_tsa_key` 命令の実装 |
| `crates/cli/src/anchor.rs` | 既存の命令構築パターン（`build_register_wasm_module_ix` 等） |
| `crates/cli/src/commands/register_wasm.rs` | 参考パターン（最もシンプルなCLIコマンド） |
| `crates/cli/src/main.rs` | サブコマンド定義 |
| `programs/title-config/README.md` | ドキュメント更新先 |
| `docs/reference.md` | ドキュメント更新先 |

## 完了条件

- [ ] `title-cli add-tsa-key` コマンド実装
- [ ] `title-cli remove-tsa-key` コマンド実装
- [ ] `anchor.rs` に命令構築ヘルパー追加
- [ ] `programs/title-config/README.md` にTSA命令を記載
- [ ] `docs/reference.md` にTSA鍵管理手順を記載
- [ ] `cargo check --workspace && cargo test --workspace` 通過
