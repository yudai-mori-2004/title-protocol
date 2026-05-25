#!/usr/bin/env bash
# Title Protocol — Build all images + EIF on EC2.
# Spec §5.4
#
# Run ON the EC2 instance from the repository root (after git clone).
# Builds the three Docker images natively (linux/amd64) and produces
# the Nitro Enclave EIF. Docker layer cache makes incremental rebuilds
# fast (1-3 min); first build takes ~25 min on c5.xlarge.
#
# Usage:
#   cd title-protocol
#   bash deploy/aws/scripts/build.sh
#
# Output: PCR0 / PCR1 / PCR2 printed at the end. Record PCR0 for
# on-chain registration via add_approved_measurement.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
EIF_NAME="${EIF_NAME:-title-protocol-tee.eif}"

cd "$REPO_ROOT"

if [[ ! -f Cargo.toml ]]; then
  echo "ERROR: run from the repository root (Cargo.toml not found)" >&2
  exit 1
fi

if ! docker info > /dev/null 2>&1; then
  echo "ERROR: Docker is not running or you're not in the docker group." >&2
  echo "       Run: sudo systemctl start docker" >&2
  echo "       If you just ran setup-host.sh, log out and back in first." >&2
  exit 1
fi

build_image() {
  local tag="$1" dockerfile="$2"
  echo ""
  echo "==> Building $tag"
  echo "    Dockerfile: $dockerfile"
  docker build --file "$dockerfile" --tag "$tag" .
}

echo "=== Title Protocol — Building images ==="
echo "    $(date)"

build_image "title-protocol-tee-nitro:latest" "deploy/aws/docker/tee-nitro.Dockerfile"
build_image "title-protocol-proxy:latest"     "deploy/aws/docker/title-proxy.Dockerfile"
build_image "title-protocol-gateway:latest"   "deploy/aws/docker/gateway.Dockerfile"

echo ""
echo "==> Built images:"
docker image ls --format 'table {{.Repository}}:{{.Tag}}\t{{.Size}}' \
  | grep -E "title-protocol-(tee-nitro|proxy|gateway)"

echo ""
echo "==> Building EIF"
sudo nitro-cli build-enclave \
  --docker-uri title-protocol-tee-nitro:latest \
  --output-file "$REPO_ROOT/$EIF_NAME"

echo ""
echo "=== Build complete at $(date) ==="
echo ""
echo "PCR values (record PCR0 for Solana registration):"
sudo nitro-cli describe-eif --eif-path "$REPO_ROOT/$EIF_NAME" | jq '.Measurements'
echo ""
echo "Next: bash deploy/aws/scripts/run.sh"
