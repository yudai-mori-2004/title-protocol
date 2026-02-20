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
echo "[Step 1/3] Docker イメージをビルド中..."
docker build -t "$IMAGE_NAME" -f "$TEE_DOCKERFILE" "$PROJECT_ROOT"
echo "  Docker イメージ: $IMAGE_NAME"

# Step 2: EIF の生成
echo "[Step 2/3] EIF を生成中..."
nitro-cli build-enclave \
    --docker-uri "$IMAGE_NAME:latest" \
    --output-file "$EIF_OUTPUT"

echo "  EIF ファイル: $EIF_OUTPUT"

# Step 3: 測定値の表示
echo "[Step 3/3] Enclave 測定値 (PCR):"
# nitro-cli build-enclave の出力にPCR値が含まれるが、
# 念のためEIFからも抽出して表示する
nitro-cli describe-eif --eif-path "$EIF_OUTPUT" | \
    python3 -c "
import sys, json
data = json.load(sys.stdin)
measurements = data.get('Measurements', {})
for key in ['PCR0', 'PCR1', 'PCR2']:
    val = measurements.get(key, 'N/A')
    print(f'  {key}: {val}')
" 2>/dev/null || echo "  (PCR値の表示にはpython3が必要です)"

echo ""
echo "=== ビルド完了 ==="
echo "EIF ファイル: $EIF_OUTPUT"
echo ""
echo "Enclave の起動:"
echo "  nitro-cli run-enclave --eif-path $EIF_OUTPUT --cpu-count 2 --memory 512"
echo ""
echo "Global Config に登録する PCR 値を上記から取得してください。"
echo "  PCR0: Enclave イメージの測定値"
echo "  PCR1: カーネルの測定値"
echo "  PCR2: アプリケーションの測定値"
