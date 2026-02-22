# タスク26: デプロイ基盤のベンダー分離

## 概要

AWS固有のインフラ・デプロイファイルを `deploy/aws/` に集約し、
プロトコルOSSとフォーク版の境界をディレクトリレベルで明確にする。

タスク25（Cargo feature flags）と合わせて、以下のOSS公開モデルが完成する:

```
title-protocol (OSS)         = deploy/aws/ なし + default-features = []
title-protocol-aws (フォーク) = deploy/aws/ あり + default-features = ["vendor-aws"]
```

## 参照

- OSS品質監査レポート §4.B「インフラ・デプロイのディレクトリ移動」
- OSS品質監査レポート §4「OSS公開フロー」

## 前提タスク

- タスク25（Cargo feature flags が導入済みであること）

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `deploy/terraform/main.tf` | 移動対象（AWS EC2 + S3 + IAM、227行） |
| `deploy/terraform/variables.tf` | 移動対象 |
| `deploy/terraform/outputs.tf` | 移動対象 |
| `deploy/terraform/user-data.sh` | 移動対象 |
| `deploy/setup-ec2.sh` | 移動対象 |
| `deploy/docker-compose.production.yml` | 移動対象 |
| `docker/tee.Dockerfile` | 移動対象（AWS Nitro EIF用） |
| `scripts/build-enclave.sh` | 移動対象（nitro-cli） |
| `docker-compose.yml` | 残留確認（ローカル開発用、ベンダー中立） |
| `docker/tee-mock.Dockerfile` | 残留確認 |

## 作業内容

### 1. deploy/aws/ ディレクトリへの集約

以下のファイルを `deploy/aws/` に移動する:

| 移動元 | 移動先 |
|--------|--------|
| `deploy/terraform/` | `deploy/aws/terraform/` |
| `deploy/setup-ec2.sh` | `deploy/aws/setup-ec2.sh` |
| `deploy/docker-compose.production.yml` | `deploy/aws/docker-compose.production.yml` |
| `docker/tee.Dockerfile` | `deploy/aws/docker/tee.Dockerfile` |
| `scripts/build-enclave.sh` | `deploy/aws/build-enclave.sh` |

### 2. 移動したファイル内の相対パス修正

移動に伴い、ファイル内の相対パス参照を修正する:

- `deploy/aws/setup-ec2.sh`: Docker Compose ファイルパス、Dockerfile パス
- `deploy/aws/build-enclave.sh`: Dockerfile の `-f` パス
- `deploy/aws/docker-compose.production.yml`: Dockerfile の `build.dockerfile` パス
- `deploy/aws/terraform/user-data.sh`: git clone 後のパス参照

### 3. 残留ファイルの確認

以下がプロトコルOSSとして残ることを確認:

```
deploy/
├── aws/               ← ベンダー固有（フォークのみ）
│   ├── terraform/
│   ├── setup-ec2.sh
│   ├── build-enclave.sh
│   ├── docker-compose.production.yml
│   └── docker/
│       └── tee.Dockerfile
└── keys/              ← 鍵管理（ベンダー中立）

docker/                ← ベンダー中立
├── tee-mock.Dockerfile
├── gateway.Dockerfile
├── proxy.Dockerfile
└── indexer.Dockerfile

scripts/               ← ベンダー中立
├── setup-local.sh
├── init-config.mjs
├── register-content.mjs
└── test-devnet.mjs
```

### 4. ドキュメント・参照の更新

移動に伴い、以下のドキュメント内のファイルパス参照を更新する:

- `README.md`: デプロイ手順のパス
- `CLAUDE.md`: タスク定義のファイルパス参照（もしあれば）
- `docs/v1/tasks/17-devnet-deploy/README.md`: setup-ec2.sh のパス参照
- `docs/v1/tasks/19-nitro-test/README.md`: 各スクリプトのパス参照

### 5. .gitignore の確認

`deploy/aws/terraform/terraform.tfstate` 等の機密ファイルが
引き続き `.gitignore` で除外されていることを確認する。

## 対象ファイル一覧

| # | ファイル | 変更 |
|---|---------|------|
| 1 | `deploy/terraform/` | **移動** → `deploy/aws/terraform/` |
| 2 | `deploy/setup-ec2.sh` | **移動** → `deploy/aws/setup-ec2.sh` |
| 3 | `deploy/docker-compose.production.yml` | **移動** → `deploy/aws/docker-compose.production.yml` |
| 4 | `docker/tee.Dockerfile` | **移動** → `deploy/aws/docker/tee.Dockerfile` |
| 5 | `scripts/build-enclave.sh` | **移動** → `deploy/aws/build-enclave.sh` |
| 6 | 上記ファイル内の相対パス | パス修正 |
| 7 | `README.md` | デプロイ手順のパス更新 |
| 8 | `CLAUDE.md` | ファイルパス参照の更新（該当箇所があれば） |

## 完了条件

- [ ] `deploy/aws/` に全AWS固有ファイルが集約されている
- [ ] `docker/` にベンダー中立なDockerfileのみ残っている（tee.Dockerfile がない）
- [ ] `scripts/` にベンダー中立なスクリプトのみ残っている（build-enclave.sh がない）
- [ ] 移動したファイル内の相対パスが正しく更新されている
- [ ] README.md / CLAUDE.md のパス参照が正しい
- [ ] `docker-compose.yml`（ローカル開発用）は変更なし
- [ ] `cargo check --workspace` 通過（Rustコードに変更がないことの確認）
