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

# プロセス名ベースのフォールバック停止（-f で部分一致）
stop_by_pattern() {
  local label="$1"
  local pattern="$2"
  local pids
  pids=$(pgrep -f "$pattern" 2>/dev/null || true)
  if [ -n "$pids" ]; then
    echo "$pids" | xargs kill 2>/dev/null || true
    echo "  停止: $label (PID=$pids, プロセス名で検出)"
  fi
}

# 各プロセスを停止（PIDファイル → プロセス名フォールバック）
stop_process "indexer"
stop_process "gateway"
stop_process "temp-storage"
stop_process "tee"

stop_by_pattern "title-gateway"      "title-gateway"
stop_by_pattern "title-temp-storage" "title-temp-storage"
stop_by_pattern "title-tee"          "title-tee"
stop_by_pattern "indexer"            "node.*indexer/dist"

# Docker Compose
echo "  Docker Compose 停止中..."
docker compose -f "$PROJECT_ROOT/deploy/local/docker-compose.yml" down 2>/dev/null || true

# 一時ファイルのクリーンアップ
rm -rf /tmp/title-uploads 2>/dev/null || true
rm -rf "$PID_DIR" 2>/dev/null || true

echo ""
echo "=== 停止完了 ==="
echo ""
