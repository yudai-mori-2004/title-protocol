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

- [x] `terraform plan` が成功する定義
- [x] docker-compose.production.yml が本番構成を定義
- [x] setup-ec2.sh がEnclave起動→init-config.mjsまでを自動化
- [x] .env.example が本番デプロイに必要な全変数を網羅

---

## 運用ノート（2026-02-21 devnetデプロイ実績）

### デプロイ成功時の環境

- EC2: `c5.xlarge` (ap-northeast-1)
- Solana devnet RPC: Helius
- Anchor Program: `C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN`
- Tree Address: `4KTZs5gT3AG9g5LhMdvEjLz2oVoQNgPDPP1e9DwgYEPE`
- TEE Signing Pubkey: `HBt4PnC4fpJWBUzvYxsEVJdX9NfmXunZ9GMmu2t5nkvg`
- Tree Config: `max_depth=14`, `max_buffer_size=64`

### 発見された問題と対策

#### 1. Merkle Treeアカウントサイズ計算の不一致（クリティカル）

**症状**: `CreateTreeConfigV2` トランザクションが `"Program failed to complete"` で失敗。

**原因**: `solana_tx.rs` の `merkle_tree_account_size()` が `@solana/spl-account-compression` SDKの `getConcurrentMerkleTreeAccountSize()` と3バイトずれていた。

- ヘッダーサイズが7バイト大きかった（V2には8バイトのdiscriminatorが存在しない）
- パスの `_padding` フィールド(4バイト)が欠落していた
- 差分: +7 - 4 = +3バイト余計にアロケート

**修正**: ヘッダーを `56` バイト（`1+1+4+4+32+8+6`）に修正、パスに `_padding(4)` を追加。

**教訓**: Solanaの `create_account` でアカウントサイズが実際のプログラム期待値と異なると、プログラムの初期化が無言で失敗する。SDKの関数で正解値を確認してからRust側を合わせること。

```
正しいサイズ値（SDKと一致）:
  (14, 64) → 31800
  (20, 64) → 44280
  (20, 1024) → 697080
```

#### 2. ComputeBudget の設定

**症状**: 大きなMerkle Treeの初期化でCompute Unit不足になる可能性がある。

**対策**: `build_create_tree_tx()` に `ComputeBudgetInstruction::set_compute_unit_limit(400_000)` を追加。デフォルト(200k CU)では不足するケースがある。

#### 3. TEEプロセスとDockerコンテナのポート競合

**症状**: `docker compose up -d tee-mock` が `port 4000 already in use` で失敗。

**原因**: 前回セッションで `nohup` でホスト上に直接起動した `title-tee` プロセスが残存。

**対策**:
```bash
# ポート4000を使っているプロセスを確認
sudo lsof -i :4000
# 残存プロセスを停止
kill <PID>
# その後 docker compose で起動
docker compose up -d tee-mock
```

**教訓**: TEEをホストで直接起動するのは避け、常に `docker compose` 経由で管理する。

#### 4. docker-compose.yml と production.yml の使い分け

- `docker-compose.yml`: `tee-mock` サービスあり（ローカル・devnet開発用）
- `deploy/docker-compose.production.yml`: `tee-mock` なし（Nitro Enclave本番用）
- devnetでモックTEEを使う場合は `docker-compose.yml` を使用する。

#### 5. TEEライフサイクルの制約

- TEEは起動時に毎回新しい鍵ペアを生成する（ステートレス設計）
- `/create-tree` は1インスタンスのライフサイクル中に**1回だけ**呼び出し可能
- TEEを再起動すると鍵が変わるため、Global Configの再登録が必要
- 再起動手順: `docker compose restart tee-mock` → `node scripts/init-config.mjs`

#### 6. Bubblegum V2 関連プログラムのdevnet存在確認

以下のプログラムは全てdevnetに存在することを確認済み（2026-02-21時点）:

| プログラム | アドレス | 役割 |
|-----------|---------|------|
| Bubblegum V2 | `BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY` | cNFT管理 |
| SPL Account Compression V2 | `mcmt6YrQEMKw8Mw43FmpRLmf7BqRnFMKmAcbxE3xkAW` | Merkle Tree |
| SPL Noop V2 | `mnoopTCrg4p8ry25e4bcWA9XZjbNjMTfgYVGGEdRsf3` | ログ記録 |

#### 7. .env の管理

コマンド内にモック値（`YOUR_RPC_URL` 等）を使わない。必ず `.env` から取得する:

```bash
source .env
solana config set --url "$SOLANA_RPC_URL"
```

### デプロイ手順チェックリスト

```
1. [ ] .env を設定（SOLANA_RPC_URL, AWS_*, COLLECTION_MINT 等）
2. [ ] source .env で環境変数をロード
3. [ ] Anchor プログラムのデプロイ確認
4. [ ] docker compose build (gateway, tee-mock, indexer)
5. [ ] ポート競合の確認 (sudo lsof -i :3000 -i :4000 -i :5000)
6. [ ] docker compose up -d
7. [ ] TEE wallet への SOL 送金
8. [ ] node scripts/init-config.mjs --rpc $SOLANA_RPC_URL
9. [ ] Tree作成成功・Global Config登録を確認
```
