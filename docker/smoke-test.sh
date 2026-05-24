#!/usr/bin/env bash
# Title Protocol — Docker smoke test
# Verifies Gateway + TEE start correctly and respond to API requests.
#
# Usage:
#   docker compose up --build -d
#   ./docker/smoke-test.sh
#   docker compose down

set -euo pipefail

GATEWAY="http://localhost:3000"
TEE="http://localhost:4000"
MAX_WAIT=60
INTERVAL=2

passed=0
failed=0

check() {
    local name="$1"
    local url="$2"
    local expected="$3"

    body=$(curl -sf "$url" 2>/dev/null) || { echo "FAIL: $name — connection refused"; failed=$((failed + 1)); return; }

    if echo "$body" | grep -q "$expected"; then
        echo "PASS: $name"
        passed=$((passed + 1))
    else
        echo "FAIL: $name — unexpected response: $body"
        failed=$((failed + 1))
    fi
}

echo "Waiting for Gateway to become ready..."
elapsed=0
while ! curl -sf "$GATEWAY/health" > /dev/null 2>&1; do
    if [ "$elapsed" -ge "$MAX_WAIT" ]; then
        echo "FAIL: Gateway did not become ready within ${MAX_WAIT}s"
        exit 1
    fi
    sleep "$INTERVAL"
    elapsed=$((elapsed + INTERVAL))
done
echo "Gateway ready (${elapsed}s)"
echo ""

check "GET /health (gateway)"       "$GATEWAY/health"       '"status":"ok"'
check "GET /health (tee)"           "$TEE/health"           '"status":"ok"'
check "GET /keys (gateway)"         "$GATEWAY/keys"         '"keys"'
check "GET /processors (gateway)"   "$GATEWAY/processors"   '"c2pa-verify"'
check "GET /solana-keys (gateway)"  "$GATEWAY/solana-keys"  '"solana_pubkey"'

echo ""
echo "Results: $passed passed, $failed failed"

if [ "$failed" -gt 0 ]; then
    exit 1
fi
