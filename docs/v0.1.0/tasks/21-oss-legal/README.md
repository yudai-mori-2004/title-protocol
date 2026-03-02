# タスク21: OSS公開ファイル整備

## 概要

オープンソースとして公開するために法的・社会的に必須なファイルを追加する。
LICENSE がないリポジトリは法的に使用不可であり、最優先で対応する。

## 参照

- OSS品質監査レポート §5「OSS公開に必要なファイル」

## 前提タスク

- タスク01〜20全完了（ただし技術的な依存はない。独立して実施可能）

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `README.md` | 既存のプロジェクト説明（整合性確認） |
| `CLAUDE.md` | 開発規約（CONTRIBUTING.md との整合性確認） |
| `.github/workflows/ci.yml` | 既存CI（cargo-audit 追加の検討） |

## 作業内容

### 1. LICENSE ファイル追加

Apache License 2.0 を選択（特許保護 + 商用利用可）。
ルートに `LICENSE` ファイルを配置する。

### 2. CONTRIBUTING.md 作成

以下の内容を含める:
- 開発環境のセットアップ手順（Rust, Node.js, Docker Compose）
- ビルド・テスト手順（`cargo check --workspace`, `cd sdk/ts && npm run build`）
- PR の作り方、ブランチ命名規約
- コーディング規約の要約（日本語docコメント、`thiserror`、仕様書§参照）
- 1タスク = 1PR の原則

### 3. SECURITY.md 作成

以下の内容を含める:
- 脆弱性の報告方法（メールアドレス or GitHub Security Advisories）
- 対象範囲（TEE, Gateway, SDK, Solana Program）
- 応答タイムライン目安

### 4. CODE_OF_CONDUCT.md 作成

Contributor Covenant v2.1 を採用。

### 5. CI に cargo-audit を追加（任意）

`.github/workflows/ci.yml` に `cargo audit` ステップを追加し、
既知の脆弱性を含む依存クレートを検出する。

## 完了条件

- [ ] `LICENSE`（Apache 2.0）がルートに存在する
- [ ] `CONTRIBUTING.md` がルートに存在し、ビルド・テスト手順が正確
- [ ] `SECURITY.md` がルートに存在する
- [ ] `CODE_OF_CONDUCT.md` がルートに存在する
- [ ] 全ファイルの内容がREADME.md / CLAUDE.md と矛盾しない
