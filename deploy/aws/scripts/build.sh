#!/usr/bin/env bash
# Title Protocol — Build all images + EIF on EC2.
# Spec §5.4 — Reproducible Build
#
# Run ON the EC2 instance from the repository root (after git clone).
# Builds the three Docker images natively (linux/amd64) and produces
# the Nitro Enclave EIF. Docker layer cache makes incremental rebuilds
# fast (1-3 min); first build takes ~25 min on c5.xlarge.
#
# Usage:
#   cd title-protocol
#   bash deploy/aws/scripts/build.sh            # normal build (uses cache)
#   bash deploy/aws/scripts/build.sh --verify   # reproducibility verify (no cache)
#
# Output: PCR0 / PCR1 / PCR2 printed at the end. Record PCR0 for
# on-chain registration via add_approved_measurement.
#
# With --verify, compares the built PCR0 against the recorded value in
# deploy/aws/build/registration/measurements.json and exits non-zero on mismatch.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
EIF_NAME="${EIF_NAME:-title-protocol-tee.eif}"
VERIFY_MODE=false
NO_CACHE=""

for arg in "$@"; do
  case "$arg" in
    --verify) VERIFY_MODE=true; NO_CACHE="--no-cache" ;;
    --no-cache) NO_CACHE="--no-cache" ;;
  esac
done

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
  docker build $NO_CACHE --file "$dockerfile" --tag "$tag" .
}

echo "=== Title Protocol — Building images ==="
echo "    $(date)"

build_image "title-protocol-tee-nitro:latest" "deploy/aws/docker/tee-nitro.Dockerfile"

if ! $VERIFY_MODE; then
  build_image "title-protocol-proxy:latest"     "deploy/aws/docker/title-proxy.Dockerfile"
  build_image "title-protocol-gateway:latest"   "deploy/aws/docker/gateway.Dockerfile"
fi

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
MEASUREMENTS=$(sudo nitro-cli describe-eif --eif-path "$REPO_ROOT/$EIF_NAME" | jq '.Measurements')
echo "$MEASUREMENTS"

if $VERIFY_MODE; then
  EXPECTED_PCR0=$(jq -r '.PCR0' "$REPO_ROOT/deploy/aws/build/registration/measurements.json")
  ACTUAL_PCR0=$(echo "$MEASUREMENTS" | jq -r '.PCR0')
  echo ""
  if [[ "$ACTUAL_PCR0" == "$EXPECTED_PCR0" ]]; then
    echo "=== REPRODUCIBILITY VERIFIED ==="
    echo "    PCR0 matches recorded value: $ACTUAL_PCR0"
  else
    echo "=== REPRODUCIBILITY FAILED ===" >&2
    echo "    Expected: $EXPECTED_PCR0" >&2
    echo "    Got:      $ACTUAL_PCR0" >&2
    echo "" >&2
    echo "    The build is NOT reproducible. Check:" >&2
    echo "      - Docker image digest pins (tee-nitro.Dockerfile)" >&2
    echo "      - Cargo.lock drift" >&2
    echo "      - rust-toolchain.toml version" >&2
    exit 1
  fi
else
  echo ""
  echo "Next: bash deploy/aws/scripts/run.sh"
  echo "To verify reproducibility: bash deploy/aws/scripts/build.sh --verify"
fi
