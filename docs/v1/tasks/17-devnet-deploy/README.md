# タスク17: Devnetデプロイ基盤

## 概要

Title Protocol をSolana devnet上で実際に運用するためのインフラ定義・デプロイ手順を整備する。

## 仕様書セクション

- §6.1 コンポーネント構成
- §6.3 Temporary Storage
- §6.4 TEE
- §8 ガバナンス

## 前提タスク

- タスク01〜16全完了（コードベースは変更不要、環境変数の切り替えのみ）

## アーキテクチャ

```
EC2 Nitro Instance (c5.xlarge)
├── Enclave: title-tee (NitroRuntime, vsock:4000)
├── Parent: title-proxy (vsock↔HTTP変換)
├── Docker: title-gateway (:3000)
├── Docker: PostgreSQL (:5432)
├── Docker: title-indexer (:5000)
└── S3 Bucket: title-uploads-devnet
```

## 成果物

| ファイル | 内容 |
|---------|------|
| `deploy/terraform/main.tf` | EC2 + S3 + SecurityGroup + IAM |
| `deploy/terraform/variables.tf` | 変数定義 |
| `deploy/terraform/outputs.tf` | 出力値（IP, S3 bucket名等） |
| `deploy/terraform/user-data.sh` | EC2起動時の初期セットアップ |
| `deploy/docker-compose.production.yml` | 本番用compose（モックサービス除外） |
| `deploy/setup-ec2.sh` | EC2上での手動セットアップ手順 |
| `.env.example` | 本番向け項目の追記 |

## 完了条件

- [ ] `terraform plan` が成功する定義
- [ ] docker-compose.production.yml が本番構成を定義
- [ ] setup-ec2.sh がEnclave起動→init-config.mjsまでを自動化
- [ ] .env.example が本番デプロイに必要な全変数を網羅
