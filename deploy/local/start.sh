#!/usr/bin/env bash
set -euo pipefail

# Title Protocol — local stack (same architecture as AWS Nitro)
#
#   Gateway (:3000) → TEE (:4000) → Proxy (:8000) → internet
#
# Usage:
#   ./deploy/local/start.sh          # build & start
#   ./deploy/local/start.sh --skip-build   # start without rebuilding
#   ./deploy/local/stop.sh           # stop all

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PID_DIR="$PROJECT_ROOT/.local-stack"
LOG_DIR="$PID_DIR/logs"

PROXY_PORT="${PROXY_PORT:-8000}"
TEE_PORT="${TEE_PORT:-4000}"
GATEWAY_PORT="${GATEWAY_PORT:-3000}"

mkdir -p "$PID_DIR" "$LOG_DIR"

# ---- helpers ----------------------------------------------------------------

log() { echo "[local-stack] $*"; }

wait_for_health() {
    local name="$1" url="$2" max_wait="${3:-30}"
    local elapsed=0
    while ! curl -sf "$url" > /dev/null 2>&1; do
        sleep 1
        elapsed=$((elapsed + 1))
        if [ "$elapsed" -ge "$max_wait" ]; then
            log "ERROR: $name did not become healthy within ${max_wait}s"
            cat "$LOG_DIR/${name}.log" 2>/dev/null | tail -20
            exit 1
        fi
    done
    log "$name healthy (${elapsed}s)"
}

cleanup_stale() {
    local name="$1"
    local pidfile="$PID_DIR/${name}.pid"
    if [ -f "$pidfile" ]; then
        local old_pid
        old_pid=$(cat "$pidfile")
        if kill -0 "$old_pid" 2>/dev/null; then
            log "Stopping stale $name (PID $old_pid)"
            kill "$old_pid" 2>/dev/null || true
            sleep 1
        fi
        rm -f "$pidfile"
    fi
}

# ---- build ------------------------------------------------------------------

if [[ "${1:-}" != "--skip-build" ]]; then
    log "Building workspace (release)..."
    cargo build --release \
        -p title-proxy \
        -p title-tee --features title-tee/runtime-mock \
        -p title-gateway \
        --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>&1 | tail -5
    log "Build complete"
fi

PROXY_BIN="$PROJECT_ROOT/target/release/title-proxy"
TEE_BIN="$PROJECT_ROOT/target/release/title-tee"
GATEWAY_BIN="$PROJECT_ROOT/target/release/title-gateway"

for bin in "$PROXY_BIN" "$TEE_BIN" "$GATEWAY_BIN"; do
    if [ ! -x "$bin" ]; then
        log "ERROR: $bin not found. Run without --skip-build."
        exit 1
    fi
done

# ---- stop stale processes ---------------------------------------------------

cleanup_stale proxy
cleanup_stale tee
cleanup_stale gateway

# ---- 1. Proxy ---------------------------------------------------------------

log "Starting proxy on TCP 127.0.0.1:$PROXY_PORT..."
PROXY_LISTEN_PORT="$PROXY_PORT" \
    RUST_LOG="${RUST_LOG:-info}" \
    "$PROXY_BIN" > "$LOG_DIR/proxy.log" 2>&1 &
echo $! > "$PID_DIR/proxy.pid"

sleep 1
if ! kill -0 "$(cat "$PID_DIR/proxy.pid")" 2>/dev/null; then
    log "ERROR: proxy failed to start"
    cat "$LOG_DIR/proxy.log" | tail -20
    exit 1
fi
log "Proxy started (PID $(cat "$PID_DIR/proxy.pid"))"

# ---- 2. TEE (mock) ----------------------------------------------------------

log "Starting TEE (mock) on 127.0.0.1:$TEE_PORT..."
TEE_RUNTIME=mock \
    TEE_BIND_ADDR="127.0.0.1:$TEE_PORT" \
    PROXY_ADDR="127.0.0.1:$PROXY_PORT" \
    RUST_LOG="${RUST_LOG:-info}" \
    "$TEE_BIN" > "$LOG_DIR/tee.log" 2>&1 &
echo $! > "$PID_DIR/tee.pid"

wait_for_health "TEE" "http://127.0.0.1:$TEE_PORT/health"

# ---- 3. Gateway --------------------------------------------------------------

log "Starting Gateway on 0.0.0.0:$GATEWAY_PORT..."
TEE_ENDPOINT="http://127.0.0.1:$TEE_PORT" \
    GATEWAY_BIND_ADDR="0.0.0.0:$GATEWAY_PORT" \
    API_KEYS="${API_KEYS:-}" \
    RUST_LOG="${RUST_LOG:-info}" \
    "$GATEWAY_BIN" > "$LOG_DIR/gateway.log" 2>&1 &
echo $! > "$PID_DIR/gateway.pid"

wait_for_health "Gateway" "http://127.0.0.1:$GATEWAY_PORT/health"

# ---- summary ----------------------------------------------------------------

log "=== Local stack running ==="
log "  Proxy:   127.0.0.1:$PROXY_PORT  (PID $(cat "$PID_DIR/proxy.pid"))"
log "  TEE:     127.0.0.1:$TEE_PORT  (PID $(cat "$PID_DIR/tee.pid"))"
log "  Gateway: 0.0.0.0:$GATEWAY_PORT  (PID $(cat "$PID_DIR/gateway.pid"))"
log ""
log "  Logs:    $LOG_DIR/"
log "  Stop:    ./deploy/local/stop.sh"
log ""
log "  Quick test:"
log "    curl http://localhost:$GATEWAY_PORT/health"
log "    curl http://localhost:$GATEWAY_PORT/processors"
