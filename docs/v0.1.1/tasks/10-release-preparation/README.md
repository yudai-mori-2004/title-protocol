# Task 10: v0.1.1 リリース準備

## 目的

全ドキュメントを初見ユーザー視点で精査し、GlobalConfig構築 → ローカルノード → EC2ノードの全フローが文書のみで再現可能な状態にする。ゼロベースの検証を経て v0.1.1 タグを確定する。

## 作業内容

### Phase 1: ドキュメント精査

初見ユーザーが以下の3フローを文書だけで完遂できるか検証する：

1. **GlobalConfig 構築フロー** (`programs/title-config/README.md`)
   - プログラムビルド・デプロイ
   - `title-cli init-global`（GlobalConfig + コレクション + WASM登録）
   - `network.json` 生成

2. **ローカルノードフロー** (`QUICKSTART.md`, `deploy/local/README.md`)
   - Phase 1 → Phase 2 の接続
   - `setup.sh` 全8ステップ（ALT含む）
   - `register-photo.ts` による検証

3. **EC2 ノードフロー** (`deploy/aws/README.md`)
   - Terraform → SSH → `setup-ec2.sh`
   - Mainnet 手動ステップ（DAO承認フロー）

精査対象ファイル：

| ファイル | 対象読者 |
|----------|----------|
| `README.md` | 全員（エントリポイント） |
| `QUICKSTART.md` | 新規来訪者 |
| `docs/architecture.md` | 全員 |
| `docs/reference.md` | オペレーター / 開発者 |
| `docs/troubleshooting.md` | オペレーター |
| `programs/title-config/README.md` | プロトコル管理者 |
| `deploy/local/README.md` | ノードオペレーター |
| `deploy/aws/README.md` | ノードオペレーター |
| `sdk/ts/README.md` | SDK利用者 |

### Phase 2: 問題リストアップ → 妥当性ダブルチェック → 修正

1. 全問題をリスト化
2. 各問題の妥当性を実コードと照合して確認
3. 確認済みの問題のみ修正

### Phase 3: ゼロベース検証

新規 Claude セッションで、ドキュメントのみを頼りに以下を実行：

1. GlobalConfig 構築（既存 devnet program ID 使用）
2. ローカルノード起動 → register-photo → broadcast → DAS確認
3. EC2 ノード起動 → 同上

### Phase 4: SDK + リリース

1. SDK を 0.1.5 に更新（npm publish）
2. devnet の公式 program ID をコード内にハードコード
3. 全テスト通過確認
4. git push → `v0.1.1` タグ

## 完了条件

- [ ] 全ドキュメントが初見ユーザーで再現可能
- [ ] ゼロベース Claude セッションで GlobalConfig → local → EC2 が通る
- [ ] SDK 0.1.5 公開
- [ ] devnet program ID がコード内にハードコード済み
- [ ] `cargo check --workspace && cargo test --workspace` 通過
- [ ] `v0.1.1` タグ付与
