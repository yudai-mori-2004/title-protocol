#!/usr/bin/env bash
# Title Protocol — Stop the full stack on EC2.
#
# Run ON the EC2 instance. Stops containers, enclave, and socat bridge.
# The EC2 instance itself keeps running.
#
# Usage:
#   bash deploy/aws/scripts/stop.sh

set -euo pipefail

echo "==> Stopping title-gateway"
sudo docker rm -f title-gateway 2>/dev/null || true

echo "==> Stopping title-proxy"
sudo docker rm -f title-proxy 2>/dev/null || true

echo "==> Stopping socat bridge"
sudo pkill -f "socat TCP-LISTEN:4000" || true

echo "==> Terminating Enclave"
sudo nitro-cli terminate-enclave --all || true

echo "Stack stopped."
