#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PID_DIR="$PROJECT_ROOT/.local-stack"

log() { echo "[local-stack] $*"; }

stop_process() {
    local name="$1"
    local pidfile="$PID_DIR/${name}.pid"
    if [ ! -f "$pidfile" ]; then
        log "$name: not running (no pidfile)"
        return
    fi
    local pid
    pid=$(cat "$pidfile")
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid"
        log "$name: stopped (PID $pid)"
    else
        log "$name: already dead (PID $pid)"
    fi
    rm -f "$pidfile"
}

# Stop in reverse order: gateway → tee → proxy
stop_process gateway
stop_process tee
stop_process proxy

log "Local stack stopped"
