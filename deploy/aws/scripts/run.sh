#!/usr/bin/env bash
# Title Protocol — Start the full TEE stack on EC2.
# Spec §5.2, §5.3
#
# Run ON the EC2 instance from the repository root.
# Starts: title-proxy → Nitro Enclave → socat bridge → title-gateway
# Idempotent: stops any running stack before starting fresh.
#
# Usage:
#   cd title-protocol
#   bash deploy/aws/scripts/run.sh
#
# Environment:
#   ENCLAVE_MEM_MIB   — Enclave memory (default: 4096)
#   ENCLAVE_CPU_COUNT  — Enclave vCPUs (default: 2)
#   ENCLAVE_DEBUG      — Set to 1 for debug-mode (zeroed PCRs; default: 0)
#   API_KEYS           — Comma-separated Bearer tokens for the Gateway (optional)
#   EIF_NAME           — EIF filename (default: title-protocol-tee.eif)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
EIF_NAME="${EIF_NAME:-title-protocol-tee.eif}"
ENCLAVE_MEM_MIB="${ENCLAVE_MEM_MIB:-4096}"
ENCLAVE_CPU_COUNT="${ENCLAVE_CPU_COUNT:-2}"
ENCLAVE_DEBUG="${ENCLAVE_DEBUG:-0}"
API_KEYS="${API_KEYS:-}"

cd "$REPO_ROOT"

if [[ ! -f "$EIF_NAME" ]]; then
  echo "ERROR: $EIF_NAME not found. Run build.sh first." >&2
  exit 1
fi

if [[ "$ENCLAVE_DEBUG" == "1" ]]; then
  DEBUG_FLAG="--debug-mode"
  echo "WARNING: ENCLAVE_DEBUG=1 — PCRs will be zeroed. Attestations are unusable for on-chain registration."
else
  DEBUG_FLAG=""
fi

echo "=== Title Protocol — Starting stack ==="
echo "    $(date)"
echo "    memory: ${ENCLAVE_MEM_MIB} MiB, cpus: ${ENCLAVE_CPU_COUNT}"
echo "    mode: $([ -n "$DEBUG_FLAG" ] && echo debug || echo release)"

echo ""
echo "==> Stopping any previous stack"
sudo docker rm -f title-gateway 2>/dev/null || true
sudo docker rm -f title-proxy 2>/dev/null || true
sudo pkill -f "socat TCP-LISTEN:4000" || true
sudo nitro-cli terminate-enclave --all || true
sleep 2

echo "==> Starting title-proxy (host network, vsock:8000)"
sudo docker run -d --name title-proxy \
  --restart unless-stopped \
  --network host \
  --privileged \
  title-protocol-proxy:latest

echo "==> Launching Nitro Enclave"
sudo nitro-cli run-enclave \
  --eif-path "$REPO_ROOT/$EIF_NAME" \
  --memory "$ENCLAVE_MEM_MIB" \
  --cpu-count "$ENCLAVE_CPU_COUNT" \
  $DEBUG_FLAG

ENCLAVE_CID=$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveCID')
echo "    Enclave CID: $ENCLAVE_CID"

echo "==> Starting socat bridge (TCP 127.0.0.1:4000 → vsock:$ENCLAVE_CID:4000)"
nohup sudo socat TCP-LISTEN:4000,reuseaddr,fork VSOCK-CONNECT:$ENCLAVE_CID:4000 \
  > "$REPO_ROOT/socat.log" 2>&1 &
SOCAT_PID=$!
sleep 1
if ! sudo kill -0 $SOCAT_PID 2>/dev/null; then
  echo "ERROR: socat exited immediately (see socat.log)" >&2
  exit 1
fi

echo "==> Waiting for TEE /health..."
TEE_READY=0
for i in $(seq 1 60); do
  if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
    echo "    TEE ready (${i}s)"
    TEE_READY=1
    break
  fi
  sleep 1
done
if [[ "$TEE_READY" != "1" ]]; then
  echo "ERROR: TEE /health did not respond within 60s." >&2
  echo "  Debug: sudo nitro-cli console --enclave-id \$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')" >&2
  echo "  Logs:  cat $REPO_ROOT/socat.log" >&2
  exit 1
fi

echo "==> Starting title-gateway (host network, port 3000)"
sudo docker run -d --name title-gateway \
  --restart unless-stopped \
  --network host \
  -e TEE_ENDPOINT=http://127.0.0.1:4000 \
  -e API_KEYS="$API_KEYS" \
  title-protocol-gateway:latest

sleep 2

echo ""
echo "=== Stack is live ==="
echo ""
sudo nitro-cli describe-enclaves | jq '.[0] | {EnclaveCID, State, CPUCount, MemoryMiB}'
sudo docker ps --format 'table {{.Names}}\t{{.Status}}' \
  --filter name=title-proxy --filter name=title-gateway
echo ""
echo "Sanity check:"
echo "  curl http://localhost:3000/health"
echo "  curl http://localhost:3000/keys | jq"
echo "  curl http://localhost:3000/solana-keys | jq"
