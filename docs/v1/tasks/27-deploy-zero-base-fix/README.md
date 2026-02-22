# Task 27: ゼロベースデプロイの一本道化

## 背景

2026-02-22 に新規EC2インスタンスでゼロからNitro TEEデプロイを実行したところ、
setup-ec2.sh を一発実行するだけでは完走せず、以下の手動介入が必要だった:

1. `gcc gcc-c++` の手動インストール（cargo build がCコンパイラ不在で失敗）
2. ホスト側 `socat` の手動インストール（Enclave↔ホストのvsockブリッジに必要）
3. IAMユーザーの手動作成 + 永続アクセスキーの発行（S3 presigned URL生成に必要）
4. Solanaウォレット作成 + SOL送金の手動実行
5. Docker グループ反映のための再ログイン

**目標:** `terraform apply` → SSH → `.env` 設定 → `./deploy/aws/setup-ec2.sh` の一本道で、
Nitro TEE全フローが完走する状態にする。

## 根本原因の分析

### A. user-data.sh のパッケージ不足

**現状:** `openssl-devel pkg-config` のみインストール。
**問題:** EC2ホスト上での `cargo build` に `gcc gcc-c++` が必要。
Enclave↔ホストのvsockブリッジに `socat` が必要。

**修正:**
- `deploy/aws/terraform/user-data.sh` に `gcc gcc-c++ socat` を追加

### B. S3認証のアーキテクチャ不整合

**現状:**
- Terraform は EC2 に IAM ロール（一時クレデンシャル）を付与
- Gateway の `S3TempStorage` は `S3_ACCESS_KEY` / `S3_SECRET_KEY` 環境変数で**明示的にキーを要求**
- IAMロールの一時クレデンシャルには `SessionToken` が必須だが、Gateway はそれに非対応

**結果:** `.env` に S3 キーを空にすると presigned URL 生成が失敗（HTTP 400/403）。

**修正方針（2案のいずれか）:**

- **案1: Terraform で IAM ユーザー + アクセスキーを作成し、outputs に含める（推奨）**
  - `main.tf` に `aws_iam_user` + `aws_iam_access_key` を追加
  - `terraform output` で `s3_access_key_id` / `s3_secret_access_key` を出力
  - ユーザーは `terraform output` の値を `.env` に貼るだけ
  - 利点: 一時クレデンシャルの期限切れ問題がない

- **案2: Gateway の S3 クライアントを AWS SDK credential chain 対応にする**
  - `S3_ACCESS_KEY` が空の場合、AWS SDK の自動検出（IAMロール）にフォールバック
  - 利点: .env にキーを書かなくて済む
  - 欠点: Rustコードの変更が必要、`rust-s3` クレートの credential chain 対応を調査要

### C. setup-ec2.sh の前提条件不足

**現状:** Solana ウォレットの存在を前提としているが、作成手順がない。

**修正:**
- setup-ec2.sh の Step 0 で `~/.config/solana/id.json` の存在チェック
- なければ `solana-keygen new` で自動作成 + 警告表示（「SOL をエアドロップまたは送金してください」）

### D. Docker グループの反映タイミング

**現状:** user-data.sh で `usermod -aG docker ec2-user` するが、
SSH初回ログイン時にはグループが反映されておらず `docker` コマンドが permission denied。

**修正:**
- user-data.sh の末尾で `newgrp docker` は効かない（非対話）
- → 手順書に「初回SSH後に `exit` → 再SSH」を明記するのみ（スクリプトでは解決不可）
- または setup-ec2.sh の先頭で `groups | grep -q docker || exec sg docker "$0"` で自動再実行

## 変更対象ファイル

| ファイル | 変更内容 |
|---------|---------|
| `deploy/aws/terraform/user-data.sh` | `gcc gcc-c++ socat` 追加 |
| `deploy/aws/terraform/main.tf` | IAMユーザー + アクセスキー追加（案1の場合） |
| `deploy/aws/terraform/outputs.tf` | `s3_access_key_id`, `s3_secret_access_key` 追加（案1の場合） |
| `deploy/aws/setup-ec2.sh` | Solanaウォレット自動作成、Docker グループチェック |
| `.env.example` | S3キーの説明を更新（「Terraform outputから取得」） |
| `docs/v1/tasks/19-nitro-test/SESSION-2026-02-22.md` | 手順書に知見を反映 |

## 読むべきファイル

- `deploy/aws/terraform/user-data.sh` — 現状のパッケージインストール
- `deploy/aws/terraform/main.tf` — IAMリソース定義
- `deploy/aws/terraform/outputs.tf` — Terraform出力値
- `deploy/aws/setup-ec2.sh` — デプロイスクリプト全体
- `crates/gateway/src/storage/` — S3クライアント実装（認証方式の確認）
- `.env.example` — 環境変数テンプレート

## 完了条件

1. 新規EC2インスタンスで以下のフローが手動介入なしで完走する:
   ```
   terraform apply
   → SSH (1回目のログインでOK、再ログイン不要が理想)
   → .env に terraform output の値を貼る
   → ./deploy/aws/setup-ec2.sh
   → 全ヘルスチェック OK
   ```
2. `experiments/register-photo.ts` が `tee_type: "aws_nitro"` で完走する
3. setup-ec2.sh 実行中にパッケージ不足エラーが出ない

## 依存関係

- Task 19（Nitro Enclave実環境テスト）の知見に基づく
- Task 17（Devnetデプロイ基盤）で作成したファイルを修正
