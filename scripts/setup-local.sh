#!/usr/bin/env bash
# Title Protocol ローカル開発環境初期化スクリプト
#
# docker-compose up 後に実行して、Solanaテストバリデータへのプログラムデプロイ等を行う。
#
# 使い方:
#   docker-compose up -d
#   ./scripts/setup-local.sh

set -euo pipefail

echo "=== Title Protocol ローカル環境セットアップ ==="

SOLANA_RPC="http://localhost:8899"
MINIO_ENDPOINT="http://localhost:9000"

# Solanaテストバリデータの準備待ち
echo "Solanaテストバリデータの起動を待機中..."
for i in $(seq 1 30); do
  if solana cluster-version --url "$SOLANA_RPC" &>/dev/null; then
    echo "Solanaテストバリデータが起動しました"
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo "ERROR: Solanaテストバリデータの起動がタイムアウトしました"
    exit 1
  fi
  sleep 2
done

# TODO: Anchorプログラムのデプロイ
# anchor deploy --provider.cluster localnet

# TODO: MinIOバケットの作成
# mc alias set local "$MINIO_ENDPOINT" minioadmin minioadmin
# mc mb local/title-temp-storage

# TODO: Global Configの初期化
# (Anchorプログラム経由でGlobal Config PDAを作成)

# TODO: テスト用Merkle Treeの作成

echo "=== セットアップ完了 ==="
