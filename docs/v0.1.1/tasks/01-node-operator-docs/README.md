# Task 01: ドキュメント体系の再設計

## 目的

現在の `QUICKSTART.md`（682行、全部入り）を Diataxis フレームワークに基づいて役割別・コンテンツ種別に分解し、各読者が最短で目的を達成できるドキュメント体系を構築する。

## 背景

### 問題 1: 3つの読者像が1ファイルに混在

現 `QUICKSTART.md` に含まれるコンテンツ：

| コンテンツ | Diataxis 種別 | 対象読者 |
|-----------|--------------|---------|
| GlobalConfig / 信頼の鎖 / オンチェーン構造 | Explanation | 全員 |
| Phase 1: プログラムデプロイ + GlobalConfig初期化 | How-to Guide | プロトコル管理者 |
| Phase 2: ノードデプロイ | How-to Guide | ノードオペレーター |
| SDK使用方法 + Register Content | Tutorial | アプリ開発者 |
| 環境変数テーブル | Reference | オペレーター |
| トラブルシューティング | How-to Guide | オペレーター |

読者の役割によって必要セクションが大きく異なり、1枚にまとまっていると情報の取捨選択が困難。

### 問題 2: RootLens E2E統合テストで発覚した具体的不備

1. **TEE環境変数 `CORE_COLLECTION_MINT` / `EXT_COLLECTION_MINT` が未設定** — setup.sh が network.json から自動設定する想定だが、手順書に明記がなく設定漏れが起きた
2. **コレクション権限委譲の自動化** — `register_tee_node` / `remove_tee_node` に MPL Core CPI を統合し、登録/削除と権限委譲/取消が1トランザクションで不可分に完了する
3. **RootLens config.ts のコレクションアドレスが不正** — network.json の値とハードコード値の不一致。ドキュメントにどの値を参照すべきかの記載がない

### 設計原則

OSS プロジェクトのドキュメント設計を調査し（Solana validator docs, Geth, Prysm, Chainlink, Gramine/EGo, Diataxis framework）、以下の原則を採用する：

1. **「QUICKSTART」は真のクイックスタートであるべき** — OSS 慣習では「5分で動かす実践チュートリアル」を意味する。概念ハブに転用しない
2. **コンテンツ種別を混在させない** — Explanation / How-to / Reference / Tutorial を別ファイルに分離
3. **アーカイブは不要** — Git 履歴が完全に残っているので旧ファイルを `docs/` に退避する必要はない。メンテされないドキュメントは害
4. **読者の動線を最短にする** — 各読者が最初に開くファイルから2クリック以内で目的の情報に到達
5. **重複を最小化する** — トラブルシューティングは1箇所に集約し、各ガイドからリンク

## 現状のファイル構成

```
title-protocol/
├── QUICKSTART.md                          ← 全部入り（682行）
├── deploy/
│   ├── local/
│   │   ├── README.md                      ← 存在するが簡易（67行）
│   │   └── setup.sh
│   └── aws/
│       ├── README.md                      ← 存在するが簡易（185行）
│       ├── setup-ec2.sh
│       └── terraform/
├── programs/
│   └── title-config/
│       └── (README.md なし)
└── docs/v0.1.0/tasks/47-quickstart-test/  ← QUICKSTARTのゼロベーステスト結果
```

## 目標のファイル構成

```
title-protocol/
├── QUICKSTART.md                          ← REWRITE: 真のクイックスタート（~150行）
│                                            devnet happy path: Phase 1 → Phase 2 (local) → verify
│                                            各ステップは詳細ガイドへのリンク付き
│
├── docs/
│   ├── architecture.md                    ← NEW: Explanation（概念説明）
│   │                                        GlobalConfig, 信頼の鎖, ウォレットの役割,
│   │                                        SOLの流れ, オンチェーン構造図
│   │
│   ├── reference.md                       ← NEW: Reference（リファレンス）
│   │                                        環境変数テーブル, network.json スキーマ,
│   │                                        CLI コマンド一覧
│   │
│   └── troubleshooting.md                 ← NEW: How-to（トラブルシューティング集約）
│                                            全環境共通 + 環境別セクション
│
├── programs/title-config/
│   └── README.md                          ← NEW: How-to（Phase 1 完全手順）
│
├── deploy/
│   ├── local/
│   │   └── README.md                      ← EXPAND: How-to（Phase 2 ローカル完全手順）
│   └── aws/
│       └── README.md                      ← EXPAND: How-to（Phase 2 AWS 完全手順）
│                                            devnet / mainnet 場合分け含む
│
└── sdk/ts/README.md                       ← 既存（変更なし、アプリ開発者の入口）
```

### 読者の動線

```
新規来訪者    → QUICKSTART.md → 5分で動かす → もっと知りたい → docs/architecture.md
ノードオペ    → deploy/local/README.md or deploy/aws/README.md → 困ったら → docs/troubleshooting.md
プロトコル管理者 → programs/title-config/README.md → Phase 1
アプリ開発者  → sdk/ts/README.md → SDK API
設定を調べたい → docs/reference.md → 環境変数、network.json、CLI
```

## 作業内容

### 1. QUICKSTART.md の書き直し（Tutorial）

現 QUICKSTART.md を真のクイックスタートとして書き直す（~150行）。

**構成：**

```markdown
# Quick Start

5分で Title Protocol のローカルノードを起動し、写真を verify する。

## 前提条件
（最小限のテーブル）

## Step 1: ネットワーク初期化（初回のみ）
  プログラムビルド → デプロイ → init-global
  （要約 + programs/title-config/README.md へのリンク）

## Step 2: ノード起動
  .env → setup.sh
  （要約 + deploy/local/README.md へのリンク）

## Step 3: 動作確認
  register-photo.ts のコマンド例

## 次のステップ
  - アーキテクチャを理解する → docs/architecture.md
  - AWS に本番ノードを立てる → deploy/aws/README.md
  - SDK でアプリを作る → sdk/ts/README.md
  - mainnet にノードを立てる → deploy/aws/README.md#mainnet
  - 環境変数・CLI リファレンス → docs/reference.md
```

### 2. docs/architecture.md の新規作成（Explanation）

現 QUICKSTART.md の概念説明セクションを独立ドキュメントとして整備。

**必須内容：**

- オンチェーン構造（GlobalConfig PDA 図、TeeNodeAccount PDA）
- GlobalConfig の全フィールド説明テーブル
- 信頼の鎖（Permissionless Protocol, Canonical Trust Root）
- Two-Phase Setup の概要図
- ウォレットの役割（Authority / Operator / TEE Internal）とSOLの流れ
- ノードアーキテクチャ（Client → Gateway → TempStorage → TEE → Solana）
- Vendor-Neutral Design の説明
- TEE ノード登録の部分署名パターン
- ノードのライフサイクル（Registration / Restart / Decommission）
- Mainnet のトラストモデル（DAO GlobalConfig が単一の信頼点）

### 3. docs/reference.md の新規作成（Reference）

**必須内容：**

- 環境変数テーブル（現 QUICKSTART の Environment Variables セクションを移植 + `.env.example` のコメントと整合）
- `network.json` のスキーマ説明（全フィールド + 誰が生成し誰が読むか）
- `title-cli` サブコマンド一覧（init-global, register-node, create-tree 等）
- ポート番号一覧（TEE:4000, Gateway:3000, TempStorage:3001, Indexer:5001, PostgreSQL:5432）

### 4. docs/troubleshooting.md の新規作成（How-to）

現 QUICKSTART のトラブルシューティングセクションを集約・拡充。

**必須内容：**

- 全環境共通
  - Port already in use
  - SOL残高不足
  - AES-GCM decryption failure（TEE key rotation）
  - Docker / PostgreSQL won't start
- ローカル固有
  - setup.sh 失敗時のリトライ手順
- AWS 固有
  - Enclave 起動失敗
  - S3 presigned URL 403
  - Proxy ログパーミッション
  - `solana: command not found`（PATH問題）
- **`CORE_COLLECTION_MINT` / `EXT_COLLECTION_MINT` の確認手順**
  - setup.sh / setup-ec2.sh が network.json から自動設定する仕組みの説明
  - `docker inspect` で環境変数を確認するコマンド例
  - 手動で設定する方法

### 5. programs/title-config/README.md の新規作成（How-to）

現 QUICKSTART の Phase 1 を独立ドキュメントとして整備。

**必須内容：**

- 前提条件（Rust, Solana CLI, cargo-build-sbf, ~5 SOL）
- Step 1: プログラムキーペア生成 + `declare_id!` 更新箇所一覧
- Step 2: ビルド（cargo-build-sbf）
- Step 3: デプロイ（solana program deploy）
- Step 4: WASM モジュールビルド
- Step 5: CLI ビルド
- Step 6: `title-cli init-global --cluster devnet`
  - 生成されるもの: `keys/authority.json`, `network.json`
  - init-global がやること一覧（コレクション作成、WASM登録、ResourceLimits設定）
- **Step 8: コレクション権限委譲（自動）**
  - `register_tee_node` が MPL Core CPI で登録と同時にコレクション権限を委譲する（`programs/title-config/README.md` Step 8 参照）
- network.json の構造と各フィールドの説明 → `docs/reference.md` へのリンク
- Phase 2（ノードデプロイ）への接続リンク

### 6. deploy/local/README.md の拡充（How-to）

現 QUICKSTART の Phase 2 ローカル部分 + 現 deploy/local/README.md を統合。

**必須内容：**

- 前提条件
- network.json が必要（Phase 1 完了済み or 提供されたものを使用）
- `.env` の設定（.env.example からのコピーと編集箇所）
- `setup.sh` の実行と各ステップの説明（7ステップ表）
- ログの確認方法
- 個別プロセスの再起動
- `teardown.sh` での停止
- integration-tests での動作確認
- トラブルシューティングは `docs/troubleshooting.md` へリンク

### 7. deploy/aws/README.md の拡充（How-to）

現 QUICKSTART の Phase 2 AWS部分 + 現 deploy/aws/README.md を統合。

**必須内容：**

- 前提条件（AWS CLI, Terraform, SSH鍵）
- Terraform でのインフラ構築（作成されるリソース表 + スケーリング）
- EC2 へのSSH + リポジトリクローン
- `.env` の設定（Terraform output → .env マッピングテーブル）
- `keys/` のコピー（authority.json + operator.json）
- `setup-ec2.sh` の実行と各ステップの説明（10ステップ表）
- **devnet と mainnet の場合分け:**
  - devnet: 自分の authority.json を使用、自動署名
  - mainnet: authority.json なし → 部分署名トランザクション出力 → DAO承認フロー
  - Mainnet の完全手順（network.json 取得 → ノードデプロイ → DAO承認 → Tree作成）
- ログの確認方法（Docker, Nitro Enclave console）
- 停止・再起動方法
- Quick test コマンド例
- トラブルシューティングは `docs/troubleshooting.md` へリンク

## 本タスクで対応済みの関連修正

### コレクション権限委譲の原子性保証

- `register_tee_node` / `remove_tee_node` Anchor 命令に MPL Core CPI を統合
- 登録と権限委譲、削除と権限取消が1トランザクションで不可分に完了する
- 不変条件: `GlobalConfig.trusted_node_keys == コレクションの UpdateDelegate.additional_delegates`

### setup-ec2.sh の環境変数書き込み

- `setup-ec2.sh` が `CORE_COLLECTION_MINT` / `EXT_COLLECTION_MINT` を `network.json` から `.env` に書き込む

## 完了条件

### ドキュメント体系

- [ ] `QUICKSTART.md` が真のクイックスタート（~150行、devnet happy path）として機能する
- [ ] `docs/architecture.md` がオンチェーン構造・信頼の鎖・ウォレットの役割を説明している
- [ ] `docs/reference.md` が環境変数・network.json・CLIコマンドを網羅している
- [ ] `docs/troubleshooting.md` が全環境のトラブルシューティングを集約している
- [ ] `programs/title-config/README.md` が Phase 1 の完全手順を含む
- [ ] `deploy/local/README.md` がローカルノードデプロイの完全手順を含む
- [ ] `deploy/aws/README.md` が AWS ノードデプロイの完全手順を含む（devnet/mainnet場合分け含む）
- [ ] コレクション権限委譲が `register_tee_node` に統合済みであることが明記されている
- [ ] `CORE_COLLECTION_MINT` / `EXT_COLLECTION_MINT` の確認手順が明記されている
- [ ] 全ドキュメント間のリンクが正しく接続されている
- [ ] QUICKSTART.md のコンテンツが漏れなく新体系に移植されている（アーカイブは作らない）

### ドキュメント再現性テスト

レポをクローン直後の状態を想定し、ドキュメントの手順だけを頼りに以下をすべて通しで実行できることを確認する。mainnet 以外の全フローが対象。

**前提:** devnet に接続可能、~5 SOL 確保済み、Prerequisites に記載のツールがインストール済み。

#### Phase 1: GlobalConfig 作成（`programs/title-config/README.md`）

- [ ] Step 1: プログラムキーペア生成が手順どおり完了する
- [ ] Step 2: `declare_id!` 更新箇所一覧が正確で、記載のファイルすべてに該当箇所がある
- [ ] Step 3: `cargo-build-sbf` でビルドが成功する
- [ ] Step 4: devnet へのプログラムデプロイが成功する
- [ ] Step 5: WASM モジュールのビルドが成功する（4モジュールすべて）
- [ ] Step 6: `title-cli` のビルドが成功する
- [ ] Step 7: `title-cli init-global --cluster devnet` が成功し、`keys/authority.json` と `network.json` が生成される
- [ ] `network.json` の全フィールドが `docs/reference.md` の記載と一致する

#### Phase 2: ローカルノード起動（`deploy/local/README.md`）

- [ ] `.env.example` → `.env` のコピーと設定がドキュメントの記載だけで完結する
- [ ] `setup.sh` が最後まで正常に完了する（7ステップすべてパス）
- [ ] TEE (:4000), Gateway (:3000), TempStorage (:3001) が応答する
- [ ] `register-node` でオンチェーン登録が成功する（コレクション権限委譲を含む）
- [ ] `create-tree` で Core + Extension Merkle Tree が作成される
- [ ] ログの確認方法（`/tmp/title-*.log`）がドキュメントどおりに機能する

#### SDK 動作確認（`QUICKSTART.md` — Verify a Photo）

- [ ] `sdk/ts` のビルドが `npm install && npm run build` で完了する
- [ ] `integration-tests/register-photo.ts --skip-sign` で verify が成功する（content_hash, protocol, provenance graph を含む結果が返る）
- [ ] `integration-tests/register-photo.ts --broadcast` で cNFT ミントまで通しで成功する

#### 横断確認

- [ ] ドキュメント間のリンク（相対パス）がすべて有効である
- [ ] ドキュメント中のコマンド例がコピー＆ペーストでそのまま動く（パスやプレースホルダが明確）
- [ ] エラー発生時に `docs/troubleshooting.md` の該当エントリで解決手順が見つかる

## 参照

- 現 QUICKSTART.md（682行、全内容）
- `docs/v0.1.0/tasks/47-quickstart-test/README.md`（ゼロベーステスト結果）
- `docs/v0.1.0/tasks/16-collection-delegate/`（コレクション権限委譲タスク）
- RootLens `document/v0.1.0/tasks/08-e2e-integration/DAS_SEARCH_FAILURE_INVESTIGATION.md`（DAS検索失敗調査 — 本タスクの発端）
- `programs/title-config/src/lib.rs`（register_tee_node / remove_tee_node に MPL Core CPI 統合済み）
- `crates/tee/src/config.rs:43-48`（TEE のコレクション環境変数読み込み）
- `.env.example`（環境変数の現状定義）
- `deploy/local/setup.sh`（ローカルセットアップスクリプト）
- `deploy/aws/setup-ec2.sh`（AWSセットアップスクリプト）
