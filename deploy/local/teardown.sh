#!/usr/bin/env bash
# Title Protocol ローカル開発 停止スクリプト
#
# setup.sh で起動した全プロセスを停止する。
#
# 使い方:
#   ./deploy/local/teardown.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
PID_DIR="/tmp/title-local"

echo "=== Title Protocol ローカル開発 停止 ==="

# PIDファイルベースの停止
stop_process() {
  local name="$1"
  local pid_file="$PID_DIR/$name.pid"

  if [ -f "$pid_file" ]; then
    local pid
    pid=$(cat "$pid_file")
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid"
      echo "  停止: $name (PID=$pid)"
    else
      echo "  スキップ: $name (PID=$pid は既に停止)"
    fi
    rm -f "$pid_file"
  else
    echo "  スキップ: $name (PIDファイルなし)"
  fi
}

# プロセス名ベースのフォールバック停止
stop_by_name() {
  local name="$1"
  local pid
  pid=$(pgrep -x "$name" 2>/dev/null || true)
  if [ -n "$pid" ]; then
    kill "$pid" 2>/dev/null || true
    echo "  停止: $name (PID=$pid, プロセス名で検出)"
  fi
}

# 各プロセスを停止
stop_process "indexer"
stop_process "gateway"
stop_process "temp-storage"
stop_process "tee"

# PIDファイルがない場合のフォールバック
stop_by_name "title-gateway"
stop_by_name "title-temp-st"
stop_by_name "title-tee"

# Indexer (node プロセス)
INDEXER_PID=$(pgrep -f "node.*indexer/dist" 2>/dev/null || true)
if [ -n "$INDEXER_PID" ]; then
  kill "$INDEXER_PID" 2>/dev/null || true
  echo "  停止: indexer (PID=$INDEXER_PID)"
fi

# Docker Compose
echo "  Docker Compose 停止中..."
docker compose -f "$PROJECT_ROOT/deploy/local/docker-compose.yml" down 2>/dev/null || true

# 一時ファイルのクリーンアップ
rm -rf /tmp/title-uploads 2>/dev/null || true
rm -rf "$PID_DIR" 2>/dev/null || true

echo ""
echo "=== 停止完了 ==="
echo ""
