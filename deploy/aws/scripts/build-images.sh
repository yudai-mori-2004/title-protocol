#!/usr/bin/env bash
# Build all three linux/amd64 Docker images needed to run the stack:
#   - title-protocol-tee-nitro   (gets turned into an EIF on EC2)
#   - title-protocol-proxy       (host-side vsock <-> internet bridge)
#   - title-protocol-gateway     (host-side public HTTP entrypoint)
#
# Runs on any host with Docker buildx (Mac/Linux). Tags are stable so
# subsequent ship steps don't need parameters.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

build() {
  local tag="$1" dockerfile="$2"
  echo ""
  echo "==> Building $tag (linux/amd64)"
  echo "    Dockerfile: $dockerfile"
  docker buildx build \
    --platform linux/amd64 \
    --file "$dockerfile" \
    --tag "$tag" \
    --load \
    .
}

build "title-protocol-tee-nitro:latest" "deploy/aws/docker/tee-nitro.Dockerfile"
build "title-protocol-proxy:latest"     "deploy/aws/docker/title-proxy.Dockerfile"
build "title-protocol-gateway:latest"   "docker/gateway.Dockerfile"

echo ""
echo "==> All images built."
docker image ls --format 'table {{.Repository}}:{{.Tag}}\t{{.Size}}' \
  | grep -E "title-protocol-(tee-nitro|proxy|gateway)"
echo ""
echo "Next: bash deploy/aws/scripts/ship-images.sh"
