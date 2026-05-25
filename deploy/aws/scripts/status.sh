#!/usr/bin/env bash
# Title Protocol — Quick status check on EC2.
#
# Usage:
#   bash deploy/aws/scripts/status.sh

set -euo pipefail

echo "=== Enclave ==="
sudo nitro-cli describe-enclaves | jq '.[0] | {EnclaveCID, State, CPUCount, MemoryMiB, Flags}' 2>/dev/null || echo "  (no enclave running)"

echo ""
echo "=== Containers ==="
sudo docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}' \
  --filter name=title-proxy --filter name=title-gateway 2>/dev/null || echo "  (none)"

echo ""
echo "=== Endpoints ==="
echo -n "  /health:      "
curl -sf http://localhost:3000/health 2>/dev/null || echo "(unreachable)"
echo ""
echo -n "  /solana-keys: "
curl -sf http://localhost:3000/solana-keys 2>/dev/null | jq -r '.solana_pubkey // "(no key)"' 2>/dev/null || echo "(unreachable)"
