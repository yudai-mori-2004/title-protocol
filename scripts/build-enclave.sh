#!/usr/bin/env bash
# Title Protocol EIF (Enclave Image File) 生成スクリプト
#
# AWS Nitro Enclaves用のEIFを生成する。
#
# 前提条件:
#   - Docker がインストール済み
#   - nitro-cli がインストール済み (Amazon Linux 2023上)
#
# 使い方:
#   ./scripts/build-enclave.sh

set -euo pipefail

echo "=== Title Protocol Enclave Image ビルド ==="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

TEE_DOCKERFILE="$PROJECT_ROOT/docker/tee.Dockerfile"
IMAGE_NAME="title-tee-enclave"
EIF_OUTPUT="$PROJECT_ROOT/title-tee.eif"

# Step 1: Docker イメージのビルド
echo "Docker イメージをビルド中..."
docker build -t "$IMAGE_NAME" -f "$TEE_DOCKERFILE" "$PROJECT_ROOT"

# Step 2: EIF の生成
echo "EIF を生成中..."
# TODO: nitro-cli build-enclave の実行
# nitro-cli build-enclave \
#   --docker-uri "$IMAGE_NAME:latest" \
#   --output-file "$EIF_OUTPUT"

echo "TODO: nitro-cli build-enclave コマンドの実行"
echo "  nitro-cli build-enclave --docker-uri $IMAGE_NAME:latest --output-file $EIF_OUTPUT"

# Step 3: 測定値の表示
# TODO: EIFの測定値 (PCR0, PCR1, PCR2) を表示
# nitro-cli describe-enclaves

echo "=== ビルド完了 ==="
