# v0.1.1 カバレッジレポート

v0.1.0 を基準とし、v0.1.1 での変更のみを追跡する。

## 変更サマリー

| カテゴリ | 変更内容 |
|---------|---------|
| ドキュメント体系 | Diataxis フレームワークに基づく7ファイル再構成 |
| Solana プログラム | `register_tee_node` / `remove_tee_node` にコレクション権限委譲を統合 |
| TEE エンドポイント | `/register-node` リクエストにコレクションアドレスを追加 |
| CLI | `register-node` / `remove-node` がコレクションアドレスを送信 |
| デプロイスクリプト | `setup-ec2.sh` の環境変数書き込み修正 |
| WASM 実行環境 | ホスト側コンテンツデコード関数 + メモリプール（セマフォ方式） |
| WASM Extension | phash-v1 を dHash → pHash (DCT) に移行、ホスト側デコード活用 |
| 仕様書 | §7.1 ホスト関数追加・メモリプール仕様、§7.4 pHash アルゴリズム更新 |

---

## ドキュメント再構成

旧 `QUICKSTART.md`（682行、全部入り）を Diataxis フレームワークで分解:

| ファイル | Diataxis 種別 | 対象読者 |
|---------|--------------|---------|
| `QUICKSTART.md` | Tutorial | 新規来訪者 |
| `docs/architecture.md` | Explanation | 全員 |
| `docs/reference.md` | Reference | オペレーター / 開発者 |
| `docs/troubleshooting.md` | How-to | オペレーター |
| `programs/title-config/README.md` | How-to | プロトコル管理者 |
| `deploy/local/README.md` | How-to | ノードオペレーター |
| `deploy/aws/README.md` | How-to | ノードオペレーター |

---

## §8 ガバナンス — コレクション権限委譲の原子化

v0.1.0 では `delegate_collection_authority` / `revoke_collection_authority` が独立した Anchor 命令として存在していた。

v0.1.1 でこれらを `register_tee_node` / `remove_tee_node` に統合し、MPL Core CPI として1トランザクション内で不可分に実行する設計に変更。

**不変条件:** `GlobalConfig.trusted_node_keys == コレクションの UpdateDelegate.additional_delegates`

| 変更前（v0.1.0） | 変更後（v0.1.1） |
|-----------------|-----------------|
| `register_tee_node` — ノード登録のみ | `register_tee_node` — ノード登録 + コレクション権限委譲（MPL Core CPI） |
| `remove_tee_node` — ノード削除のみ | `remove_tee_node` — ノード削除 + コレクション権限取消（MPL Core CPI） |
| `delegate_collection_authority` — 独立命令 | 削除（register_tee_node に統合） |
| `revoke_collection_authority` — 独立命令 | 削除（remove_tee_node に統合） |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `programs/title-config/src/lib.rs` | CPI ヘルパー追加、RegisterTeeNode / RemoveTeeNode コンテキストにコレクションアカウント追加 |
| `crates/types/src/lib.rs` | `RegisterNodeRequest` に `core_collection_mint` / `ext_collection_mint` 追加 |
| `crates/tee/src/endpoints/register_node.rs` | コレクションアドレスのパース、命令アカウント 5→8 に拡張 |
| `crates/cli/src/commands/register_node.rs` | `network.json` からコレクションアドレスを送信 |
| `crates/cli/src/commands/remove_node.rs` | コレクションアドレスを `build_remove_tee_node_ix` に渡す |
| `crates/cli/src/anchor.rs` | `build_remove_tee_node_ix` にコレクションアカウント追加、独立命令ビルダー削除 |

---

## デプロイスクリプト修正

| ファイル | 変更内容 |
|---------|---------|
| `deploy/aws/setup-ec2.sh` | `ensure_env "CORE_COLLECTION_MINT"` / `ensure_env "EXT_COLLECTION_MINT"` を追加。`network.json` の値を `.env` に書き込む |

---

## タスク一覧

| タスク | 内容 | 状態 |
|-------|------|------|
| [01-node-operator-docs](tasks/01-node-operator-docs/README.md) | ドキュメント体系再設計 + コレクション権限委譲統合 + 環境変数修正 | ドキュメント再現性テスト以外完了 |
| [02-wasm-decode-host](tasks/02-wasm-decode-host/README.md) | WASM ホスト側デコード + メモリプール + pHash (DCT) | 完了 |
