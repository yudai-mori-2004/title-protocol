# タスク30: OSS公開リリース準備

## 概要

Title Protocolをオープンソースとして公開するために必要な**全ての準備**を行う。
ライセンス・法的ファイル、パッケージメタデータ、npm publish基盤、
ノード運用ドキュメント、GlobalConfig管理ガイド、CI/CD強化を含む。

**タスク21（OSSファイル整備）を吸収する。** タスク21で定義された成果物は本タスクの一部として実装する。

## 方針

- このレポを GitHub public にした瞬間から、第三者が
  (a) ライセンスを確認でき、(b) ビルド・テストでき、(c) ノードを立てられ、
  (d) SDK を npm install でき、(e) GlobalConfig の運用方法を理解できる状態にする
- 既存の散在したドキュメント（Task 1~30, 等）は消さない。
  それらを参照しつつ、公開向けの整理されたドキュメントを新規作成する

## 前提タスク

- タスク01〜29 全完了（技術的には独立して実施可能）

## 仕様書セクション

- §5.2 Step 1: Global Config
- §8: title-config プログラム
- §6.7: TypeScript SDK

---

## A. OSS法的ファイル（タスク21の吸収）

### A-1. LICENSE

- **Apache License 2.0** をリポジトリルートに配置
- 特許保護 + 商用利用可。Solana エコシステムでは標準的

### A-2. CONTRIBUTING.md

以下の内容を含める:

| セクション | 内容 |
|-----------|------|
| 前提環境 | Rust 1.82+, Node.js 20+, Docker Compose, Solana CLI |
| ビルド手順 | `cargo check --workspace`, WASM個別ビルド, SDK/Indexerの`npm run build` |
| テスト手順 | `cargo test --workspace`, `node --test`, E2Eテスト |
| PR作成ルール | ブランチ命名規約、1タスク=1PR原則 |
| コーディング規約要約 | 日本語docコメント + 仕様書§参照、`thiserror`、`#![no_std]` (WASM) |
| AI駆動開発 | CLAUDE.md の説明、docs/ のバージョニング構造 |

### A-3. SECURITY.md

| セクション | 内容 |
|-----------|------|
| 報告方法 | GitHub Security Advisories を推奨（メールアドレスも併記） |
| 対象スコープ | TEE (crates/tee), Gateway (crates/gateway), Proxy (crates/proxy), SDK (sdk/ts), Solana Program (programs/title-config), 暗号処理 (crates/crypto) |
| 対象外 | prototype/, experiments/, docs/ |
| 応答タイムライン | 受領確認: 48時間以内、トリアージ: 7日以内、修正: 深刻度による |

### A-4. CODE_OF_CONDUCT.md

- Contributor Covenant v2.1 を採用
- 連絡先をプロジェクトのメールアドレスに設定

### 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `docs/v0.1.0/tasks/21-oss-legal/README.md` | 元タスクの要件定義 |
| `README.md` | 既存内容との整合性確認 |
| `CLAUDE.md` | CONTRIBUTING.md との整合性確認 |

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `LICENSE` | 新規作成 |
| `CONTRIBUTING.md` | 新規作成 |
| `SECURITY.md` | 新規作成 |
| `CODE_OF_CONDUCT.md` | 新規作成 |

---

## B. パッケージメタデータの整備

### B-1. 全 Cargo.toml に共通メタデータ追加

対象: 7 workspace クレート + 4 WASM モジュール + 1 Anchor プログラム = **12ファイル**

```toml
license = "Apache-2.0"
repository = "https://github.com/<org>/title-protocol"
homepage = "https://github.com/<org>/title-protocol"
authors = ["Title Protocol Contributors"]
```

**注意**: `<org>` は公開時の GitHub Organization 名に置き換える。
workspace 共通設定として `[workspace.package]` に集約し、各クレートでは `license.workspace = true` 等で継承するのが望ましい。

対象ファイル一覧:

| ファイル | 現状の name |
|---------|------------|
| `crates/types/Cargo.toml` | `title-types` |
| `crates/crypto/Cargo.toml` | `title-crypto` |
| `crates/core/Cargo.toml` | `title-core` |
| `crates/wasm-host/Cargo.toml` | `title-wasm-host` |
| `crates/tee/Cargo.toml` | `title-tee` |
| `crates/gateway/Cargo.toml` | `title-gateway` |
| `crates/proxy/Cargo.toml` | `title-proxy` |
| `wasm/phash-v1/Cargo.toml` | `title-phash-v1` |
| `wasm/hardware-google/Cargo.toml` | `title-hardware-google` |
| `wasm/c2pa-training-v1/Cargo.toml` | `title-c2pa-training-v1` |
| `wasm/c2pa-license-v1/Cargo.toml` | `title-c2pa-license-v1` |
| `programs/title-config/Cargo.toml` | `title-config` |

### B-2. package.json メタデータ追加

**`sdk/ts/package.json`** に以下を追加:

```json
{
  "license": "Apache-2.0",
  "repository": {
    "type": "git",
    "url": "https://github.com/<org>/title-protocol.git",
    "directory": "sdk/ts"
  },
  "homepage": "https://github.com/<org>/title-protocol/tree/main/sdk/ts",
  "keywords": ["solana", "title-protocol", "c2pa", "tee", "cnft", "attribution"],
  "files": ["dist", "!dist/__tests__"],
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "default": "./dist/index.js"
    }
  },
  "publishConfig": {
    "access": "public"
  }
}
```

**`indexer/package.json`** に同様の追加 + `"types": "dist/index.d.ts"` を追加。

**ルート `package.json`** に `"license": "Apache-2.0"` を追加（private: true のままでOK）。

### B-3. SDK dist/ のクリーンアップ

削除済みソースファイル（register.ts, storage.ts, resolve.ts, discover.ts）のコンパイル済みアーティファクトが `sdk/ts/dist/` に残っている。

- `npm run clean && npm run build` で再ビルド
- `.npmignore` を作成し、テストファイルを除外:
  ```
  src/
  __tests__/
  *.test.js
  *.test.d.ts
  tsconfig.json
  ```

### 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `Cargo.toml` (ルート) | workspace 設定の確認 |
| `sdk/ts/package.json` | 現状のフィールド確認 |
| `indexer/package.json` | 現状のフィールド確認 |
| `package.json` (ルート) | workspaces 設定の確認 |

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `Cargo.toml` (ルート) | `[workspace.package]` 追加 |
| 12 個の Cargo.toml | license, repository 等追加 |
| `sdk/ts/package.json` | メタデータ追加 |
| `indexer/package.json` | メタデータ + types 追加 |
| `package.json` (ルート) | license 追加 |
| `sdk/ts/.npmignore` | 新規作成 |
| `indexer/.npmignore` | 新規作成 |

---

## C. npm publish 基盤

### C-1. パッケージ README

npm パッケージには独自の README が必要（npm レジストリに表示される）。

**`sdk/ts/README.md`** — 以下を含める:

| セクション | 内容 |
|-----------|------|
| 概要 | Title Protocol TypeScript SDK の役割 |
| インストール | `npm install @title-protocol/sdk` |
| クイックスタート | TitleClient の基本的な使い方（selectNode → upload → verify → sign） |
| API一覧 | TitleClient のメソッド一覧と簡潔な説明 |
| 暗号化 | E2EE (X25519 + AES-256-GCM) の概要とcrypto関数の使い方 |
| 型定義 | 主要な型（VerifyRequest, SignRequest, NodeInfo 等）の説明 |

**`indexer/README.md`** — 以下を含める:

| セクション | 内容 |
|-----------|------|
| 概要 | cNFT インデクサの役割（Webhook + Poller + PostgreSQL） |
| 環境変数 | DATABASE_URL, DAS_ENDPOINTS, COLLECTION_MINTS, POLL_INTERVAL_MS, WEBHOOK_PORT |
| セットアップ | PostgreSQL + `npm start` |
| ライブラリとしての使用 | IndexerDb, DasClient のインポート例 |

### C-2. GitHub Actions npm publish ワークフロー

`.github/workflows/publish.yml` を新規作成。
Git tag (`v*`) の push をトリガーに、SDK と Indexer を npm に publish する。

```yaml
name: Publish to npm
on:
  push:
    tags: ['v*']
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          registry-url: 'https://registry.npmjs.org'
      - name: Build & Publish SDK
        working-directory: sdk/ts
        run: |
          npm ci
          npm run build
          npm publish --access public
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
      - name: Build & Publish Indexer
        working-directory: indexer
        run: |
          npm ci
          npm run build
          npm publish --access public
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
```

**前提**: GitHub リポジトリの Settings → Secrets に `NPM_TOKEN` を設定する。
`@title-protocol` npm organization を事前に作成し、スコープを確保しておく。

### C-3. @title-protocol npm organization 確保

`https://www.npmjs.com/org/title-protocol` を作成する（無料プランで可）。
`@title-protocol/sdk` と `@title-protocol/indexer` のスコープを確保。

### 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `sdk/ts/src/index.ts` | 公開APIの確認 |
| `sdk/ts/src/client.ts` | TitleClient のメソッド一覧 |
| `indexer/src/index.ts` | エクスポート一覧、環境変数 |
| `.github/workflows/ci.yml` | 既存CIとの整合性 |

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `sdk/ts/README.md` | 新規作成 |
| `indexer/README.md` | 新規作成 |
| `.github/workflows/publish.yml` | 新規作成 |

---

## D. CI/CD 強化

### D-1. cargo-audit の追加

`.github/workflows/ci.yml` の `rust-check` ジョブに `cargo-audit` ステップを追加する。

```yaml
- name: Install cargo-audit
  run: cargo install cargo-audit
- name: Security audit
  run: cargo audit
```

既知の脆弱性を含む依存クレートを早期検出する。

### D-2. WASM ハッシュの自動検証（任意・将来）

WASM モジュールのビルド後に SHA-256 ハッシュを計算し、
GlobalConfig に登録済みのハッシュと照合するステップ。
（v1 では手動管理のため、任意とする）

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `.github/workflows/ci.yml` | cargo-audit ステップ追加 |

---

## E. GlobalConfig 運用ガイド

### E-1. docs/v0.1.0/GLOBALCONFIG-GUIDE.md 新規作成

v1 における GlobalConfig の管理方法を一元的にまとめたドキュメント。
既存の `docs/v0.1.0/tasks/29-globalconfig-devnet/mainnet-guide.md` を参照しつつ、
**運用者向けの日常操作手順**に焦点を当てる。

以下の内容を含める:

#### 前提

- GlobalConfig PDA はネットワークごとに唯一（devnet: 1つ、mainnet: 1つ）
- 全ての更新操作に authority keypair の署名が必要
- v1 では単一keypair管理。将来的に Squads 等の multi-sig に移行予定

#### GlobalConfig のフィールド

| フィールド | 型 | 更新命令 |
|-----------|---|---------|
| `authority` | Pubkey | 初期化時に固定（変更不可） |
| `core_collection_mint` | Pubkey | `update_collections` |
| `ext_collection_mint` | Pubkey | `update_collections` |
| `trusted_tee_nodes` | Vec\<TrustedTeeNodeAccount\> | `update_tee_nodes` |
| `trusted_wasm_modules` | Vec\<TrustedWasmModuleAccount\> | `update_wasm_modules` |
| `trusted_tsa_keys` | Vec\<[u8; 32]\> | `update_tsa_keys` |

#### TrustedTeeNodeAccount の構造

```
signing_pubkey:    [u8; 32]  — TEE の Ed25519 公開鍵（署名用）
encryption_pubkey: [u8; 32]  — TEE の X25519 公開鍵（E2EE用）
gateway_pubkey:    [u8; 32]  — Gateway の Ed25519 公開鍵（認証用）
status:            u8        — 0=Inactive, 1=Active
tee_type:          u8        — 0=aws_nitro, 1=amd_sev_snp, 2=intel_tdx
```

#### ノード追加手順

1. ノードを起動し、Gateway の `/.well-known/title-node-info` から `gateway_pubkey` を取得
2. TEE の `signing_pubkey` と `encryption_pubkey` を取得（起動ログ or `tee-info.json`）
3. Authority keypair で `update_tee_nodes` を実行（既存ノードリスト + 新ノードを含む完全なリストを渡す）
4. `delegate_collection_authority` を実行（Core + Extension 両コレクション）
5. TEE で `/create-tree` を呼び出して Merkle Tree を作成

**注意**: `update_tee_nodes` は**リスト全体を置き換える**。追加時は既存ノードも含めて渡すこと。

#### ノード削除手順

**正常停止の場合:**
1. ノードを停止（TEE の秘密鍵はメモリから消滅）
2. Authority keypair で `update_tee_nodes` を実行（該当ノードを除外したリストを渡す）
3. 既存の cNFT はそのまま有効

**不正検知の場合:**
1. `revoke_collection_authority` を実行
2. 必要に応じて `unverify` で当該ノードがミントした cNFT を無効化
3. `update_tee_nodes` でノードを除外

#### TEE 再起動時の対応

TEE はステートレス設計のため、再起動すると鍵が再生成される。

1. 新しい `signing_pubkey` / `encryption_pubkey` を取得
2. `update_tee_nodes` で GlobalConfig を更新
3. `delegate_collection_authority` を再実行
4. `/create-tree` で新しい Merkle Tree を作成

**v1 における制限**: ノードの増減・再起動のたびに authority による手動更新が必要。
ノード数が少ないうちはこの運用で問題ない。
将来的にはノード管理用スマートコントラクト（ノードの自己登録 + TEE Attestation による自動承認）を検討する。

#### WASMモジュール更新手順

1. WASM モジュールを再ビルド
2. SHA-256 ハッシュを計算
3. `update_wasm_modules` を実行（全モジュールのリストを渡す）

#### スクリプトリファレンス

| スクリプト | 用途 |
|-----------|------|
| `scripts/init-devnet.mjs` | GlobalConfig 初期化 + ノード登録 + WASM登録 + Collection Authority委譲 + Merkle Tree作成（冪等） |

```bash
# 使用例: devnet
cd scripts && npm install
node init-devnet.mjs --rpc https://api.devnet.solana.com --gateway http://<IP>:3000

# 使用例: ノード情報のみ更新（Tree作成スキップ）
node init-devnet.mjs --rpc https://api.devnet.solana.com --gateway http://<IP>:3000 --skip-tree
```

### 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `programs/title-config/src/lib.rs` | 全命令の実装 |
| `scripts/init-devnet.mjs` | 運用スクリプトの実装 |
| `docs/v0.1.0/tasks/29-globalconfig-devnet/mainnet-guide.md` | 既存の構築ガイド |
| `docs/v0.1.0/SPECS_JA.md` §5.2, §8 | 仕様 |

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `docs/v0.1.0/GLOBALCONFIG-GUIDE.md` | 新規作成 |

---

## F. ノードデプロイメントガイド

### F-1. deploy/aws/README.md 新規作成

**空っぽの AWS アカウントから Title Protocol の AWS Nitro ノードを立てるための完全な手順書。**
OSS としての責任を果たすため、外部の人間がこのドキュメントだけで再現可能な粒度で記述する。

以下のセクションを含める:

#### アーキテクチャ概要

```
                         ┌─── EC2 Instance (c5.xlarge) ───────────────────┐
                         │                                                 │
Internet ──:3000──→ Docker │  Gateway  │──:4000──→ socat ──vsock──→ ┌──────┐│
                         │  Indexer   │                              │ TEE  ││
                         │  Postgres  │  ←──vsock──socat──:8000──── │(EIF) ││
                         │  (compose) │                              └──────┘│
                         │                                                 │
                         │  S3 (temp storage) ←── IAM User credentials     │
                         └─────────────────────────────────────────────────┘
```

#### 前提条件

- AWS アカウント（IAM ユーザー、AdministratorAccess 推奨）
- AWS CLI 設定済み
- Terraform 1.5+ インストール済み
- SSH キーペア（EC2 接続用）

#### Step 1: SSH キーペアの準備

```bash
mkdir -p deploy/aws/keys
aws ec2 create-key-pair --key-name title-protocol-devnet \
  --query 'KeyMaterial' --output text > deploy/aws/keys/title-protocol-devnet.pem
chmod 400 deploy/aws/keys/title-protocol-devnet.pem
```

#### Step 2: Terraform による AWS リソース作成

```bash
cd deploy/aws/terraform
terraform init
terraform plan
terraform apply
```

作成されるリソース:
- EC2 (c5.xlarge, Nitro Enclave 対応, Amazon Linux 2023)
- S3 バケット（一時ストレージ、1日ライフサイクル）
- IAM ユーザー + アクセスキー（S3 認証用）
- Security Group（SSH:22, Gateway:3000, Indexer:5000）

#### Step 3: .env の設定

```bash
# Terraform 出力値の取得
terraform output s3_access_key_id
terraform output s3_secret_access_key
terraform output s3_bucket_name
terraform output instance_public_ip
```

`.env.example` をコピーし、Terraform 出力値と Solana RPC URL を設定。

#### Step 4: EC2 に SSH → リポジトリクローン → setup-ec2.sh 実行

```bash
# SSH
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@$(terraform output -raw instance_public_ip)

# EC2上で
git clone <REPO_URL> ~/title-protocol
cd ~/title-protocol
cp .env.example .env
# .env を編集（Terraform output の値を貼る）
./deploy/aws/setup-ec2.sh
```

`setup-ec2.sh` が自動で実行する内容:
1. .env の検証
2. WASM モジュール 4 個のビルド
3. ホスト側バイナリ（proxy）のビルド
4. TEE Docker イメージ → EIF (Enclave Image File) のビルド
5. Nitro Enclave の起動 + vsock ブリッジ設定
6. Proxy の起動
7. Docker Compose（Gateway + Indexer + PostgreSQL）の起動
8. S3 アクセスの検証
9. GlobalConfig 初期化（init-devnet.mjs）
10. ヘルスチェック

#### Step 5: GlobalConfig の初期化

setup-ec2.sh のStep 7 で自動実行されるが、手動で再実行も可能:

```bash
cd scripts
node init-devnet.mjs --rpc $SOLANA_RPC_URL --gateway http://localhost:3000
```

#### Step 6: 動作確認

```bash
# Gateway ヘルスチェック
curl http://<IP>:3000/.well-known/title-node-info

# ストレステスト（experiments/から）
npx tsx stress-test.ts <IP> ../tests/e2e/fixtures/signed.jpg \
  --wallet ../deploy/aws/keys/devnet-authority.json \
  --encryption-pubkey "<encryption_pubkey_base64>"
```

#### Terraform 変数リファレンス

| 変数 | デフォルト | 説明 |
|------|----------|------|
| `aws_region` | `ap-northeast-1` | AWSリージョン |
| `instance_type` | `c5.xlarge` | EC2インスタンスタイプ（Nitro対応必須） |
| `key_name` | `title-protocol-devnet` | EC2キーペア名 |
| `enclave_cpu_count` | `2` | Enclave割当vCPU |
| `enclave_memory_mib` | `1024` | Enclave割当メモリ(MiB) |
| `s3_bucket_name` | `title-uploads-devnet` | S3バケット名 |
| `volume_size` | `50` | EBSボリュームサイズ(GB) |

#### トラブルシューティング

既存の `docs/v0.1.0/tasks/17-devnet-deploy/README.md` と `docs/v0.1.0/tasks/27-deploy-zero-base-fix/README.md` に詳細な罠・解決策が記載されている。主なものを抜粋・統合する。

#### ノードの停止と再起動

```bash
# Enclave の停止
nitro-cli terminate-enclave --all

# 全サービスの停止
docker compose -f deploy/aws/docker-compose.production.yml down

# 再起動時は setup-ec2.sh を再実行
# TEE の鍵が再生成されるため、GlobalConfig の更新も必要
```

### 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `deploy/aws/setup-ec2.sh` | メインデプロイスクリプト |
| `deploy/aws/terraform/main.tf` | Terraform リソース定義 |
| `deploy/aws/terraform/variables.tf` | Terraform 変数 |
| `deploy/aws/terraform/outputs.tf` | Terraform 出力値 |
| `deploy/aws/terraform/user-data.sh` | EC2 初期化スクリプト |
| `deploy/aws/docker/tee.Dockerfile` | TEE コンテナ定義 |
| `deploy/aws/docker/entrypoint.sh` | Enclave 起動スクリプト |
| `deploy/aws/docker-compose.production.yml` | 本番 Docker Compose |
| `deploy/aws/build-enclave.sh` | EIF ビルドスクリプト |
| `.env.example` | 環境変数テンプレート |
| `docs/v0.1.0/tasks/17-devnet-deploy/README.md` | 既存のデプロイ知見 |
| `docs/v0.1.0/tasks/27-deploy-zero-base-fix/README.md` | ゼロベースデプロイの罠 |

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `deploy/aws/README.md` | 新規作成 |

---

## G. .gitignore 監査と機密情報チェック

### G-1. 現在の .gitignore 状況

現在の `.gitignore` は十分にカバーしている:
- `deploy/aws/keys/` — SSH鍵、Authority keypair
- `*.pem` — SSL/SSH秘密鍵
- `.env` / `.env.*` — 環境変数（`.env.example` は除外）
- `deploy/aws/terraform/*.tfstate*` — Terraform状態ファイル
- `deploy/aws/terraform/.terraform/` — Terraformプラグイン
- `*.eif` — Enclave Image File

### G-2. 追加すべき項目

| パターン | 理由 |
|---------|------|
| `deploy/aws/terraform/tfplan` | `terraform plan -out=tfplan` の出力。現在 untracked で残っている |

### G-3. 公開前の最終チェック

`git log` で過去のコミットに機密情報が含まれていないか確認する:
- keypair JSON ファイルが過去にコミットされていないか
- `.env` が過去にコミットされていないか
- AWS アクセスキー、シークレットキーがコードにハードコードされていないか

**注意**: `deploy/aws/keys/devnet-authority.json` は `.gitignore` で除外されているが、
過去にコミットされていた場合は `git filter-branch` 等で履歴から除去が必要。

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `.gitignore` | `deploy/aws/terraform/tfplan` 追加 |

---

## H. README.md の更新

### H-1. ルート README.md

以下の点を修正・追加:

| 修正箇所 | 内容 |
|---------|------|
| License セクション | LICENSE ファイル作成後、正しいライセンス名（Apache-2.0）を明記 |
| Repository Structure | `tasks/` の説明を `01-30` に更新 |
| Deploying to Devnet | `deploy/aws/README.md` へのリンクを追加 |
| Contributing | CONTRIBUTING.md へのリンクを追加 |
| Security | SECURITY.md へのリンクを追加 |

### H-2. CLAUDE.md

タスク一覧にタスク30を追加:

```
| 30 | `docs/v0.1.0/tasks/30-oss-release-readiness/` | OSS公開リリース準備 | 未着手 |
```

### 変更対象ファイル

| ファイル | 操作 |
|---------|------|
| `README.md` | 修正 |
| `CLAUDE.md` | タスク30 追加 |

---

## 変更対象ファイル 総まとめ

### 新規作成（11ファイル）

| ファイル | 内容 |
|---------|------|
| `LICENSE` | Apache License 2.0 |
| `CONTRIBUTING.md` | コントリビューションガイド |
| `SECURITY.md` | 脆弱性報告ポリシー |
| `CODE_OF_CONDUCT.md` | Contributor Covenant v2.1 |
| `docs/v0.1.0/GLOBALCONFIG-GUIDE.md` | GlobalConfig 運用ガイド |
| `deploy/aws/README.md` | AWS Nitro ノードデプロイ手順書 |
| `sdk/ts/README.md` | SDK パッケージ README |
| `indexer/README.md` | Indexer パッケージ README |
| `sdk/ts/.npmignore` | npm publish 除外設定 |
| `indexer/.npmignore` | npm publish 除外設定 |
| `.github/workflows/publish.yml` | npm publish ワークフロー |

### 修正（18ファイル）

| ファイル | 変更内容 |
|---------|---------|
| `Cargo.toml` (ルート) | `[workspace.package]` 追加 |
| `crates/types/Cargo.toml` | メタデータ追加 |
| `crates/crypto/Cargo.toml` | メタデータ追加 |
| `crates/core/Cargo.toml` | メタデータ追加 |
| `crates/wasm-host/Cargo.toml` | メタデータ追加 |
| `crates/tee/Cargo.toml` | メタデータ追加 |
| `crates/gateway/Cargo.toml` | メタデータ追加 |
| `crates/proxy/Cargo.toml` | メタデータ追加 |
| `wasm/phash-v1/Cargo.toml` | メタデータ追加 |
| `wasm/hardware-google/Cargo.toml` | メタデータ追加 |
| `wasm/c2pa-training-v1/Cargo.toml` | メタデータ追加 |
| `wasm/c2pa-license-v1/Cargo.toml` | メタデータ追加 |
| `programs/title-config/Cargo.toml` | メタデータ追加 |
| `sdk/ts/package.json` | メタデータ追加 |
| `indexer/package.json` | メタデータ + types 追加 |
| `package.json` (ルート) | license 追加 |
| `.gitignore` | tfplan 追加 |
| `.github/workflows/ci.yml` | cargo-audit 追加 |
| `README.md` | License・Contributing・Security リンク追加 |
| `CLAUDE.md` | タスク30 追加 |

---

## 完了条件

### 法的ファイル
- [ ] `LICENSE`（Apache 2.0）がルートに存在する
- [ ] `CONTRIBUTING.md` がルートに存在し、ビルド・テスト手順が正確
- [ ] `SECURITY.md` がルートに存在し、報告方法・スコープ・タイムラインが明記されている
- [ ] `CODE_OF_CONDUCT.md` がルートに存在する

### パッケージメタデータ
- [ ] 全 12 個の Cargo.toml に `license`, `repository` が設定されている
- [ ] `sdk/ts/package.json` に `license`, `repository`, `files`, `publishConfig` が設定されている
- [ ] `indexer/package.json` に `license`, `repository`, `types`, `files`, `publishConfig` が設定されている
- [ ] `sdk/ts/dist/` にステイルなアーティファクトがない（clean build 済み）

### npm publish
- [ ] `sdk/ts/README.md` が存在し、API ドキュメントを含む
- [ ] `indexer/README.md` が存在し、セットアップ手順を含む
- [ ] `.github/workflows/publish.yml` が存在し、tag push で publish される
- [ ] `@title-protocol` npm organization が作成済み

### ドキュメント
- [ ] `docs/v0.1.0/GLOBALCONFIG-GUIDE.md` が存在し、ノード追加/削除/WASM更新の手順が明記されている
- [ ] `deploy/aws/README.md` が存在し、ゼロから AWS Nitro ノードを構築できる粒度で記述されている

### CI/CD
- [ ] `.github/workflows/ci.yml` に `cargo audit` ステップが存在する

### 機密情報
- [ ] `.gitignore` に `deploy/aws/terraform/tfplan` が含まれている
- [ ] `git log` で機密情報の漏洩がないことを確認済み

### 整合性
- [ ] `README.md` が LICENSE, CONTRIBUTING, SECURITY へのリンクを含む
- [ ] `CLAUDE.md` のタスク一覧にタスク30が含まれる
- [ ] `cargo check --workspace && cargo test --workspace` が通る
- [ ] `cd sdk/ts && npm run build` が通る
- [ ] `cd indexer && npm run build` が通る

---

## 実施順序の推奨

1. **A: 法的ファイル** — LICENSE が最優先（これがないと法的に使用不可）
2. **B: Cargo.toml / package.json** — 機械的な変更、先に片付ける
3. **G: .gitignore 監査** — 公開前の安全確認
4. **E: GlobalConfig ガイド** — 既存の mainnet-guide.md をベースに整理
5. **F: deploy/aws README** — 既存スクリプトの説明を統合
6. **C: npm publish 基盤** — SDK/Indexer README + publish ワークフロー
7. **D: CI/CD 強化** — cargo-audit 追加
8. **H: README/CLAUDE.md 更新** — 最後に全体の整合性を取る

---

## 備考

- `<org>` は公開時の GitHub Organization 名に置き換える。現時点では未定の場合はプレースホルダのままにし、公開直前に一括置換する
- `@title-protocol` npm スコープの確保は早めに行うこと（先着順）
- mainnet-guide.md に記載の multi-sig への移行は v2 以降のタスクとする
- ノード管理用スマートコントラクト（自己登録 + Attestation 自動承認）も v2 以降
