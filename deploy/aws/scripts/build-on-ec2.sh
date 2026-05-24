#!/usr/bin/env bash
# Build all three images on the EC2 host (native linux/amd64) and produce
# the Nitro Enclave EIF in one step. This is the reproducible path that
# Spec §5.4 calls for: anyone with the same source tree and the same
# Amazon Linux 2023 AMI gets the same PCR0.
#
# Compared with `build-images.sh + ship-images.sh` (which cross-builds on
# the operator's Mac), this script:
#   - guarantees the build environment matches what verifiers will use
#   - avoids the ~300 MB `docker save | scp | docker load` transfer round
#   - takes longer on first run (~25 min on c5.xlarge); incremental
#     rebuilds use Docker's layer cache and finish in 1-3 min
#
# Usage:
#   bash deploy/aws/scripts/build-on-ec2.sh
#
# Output: PCR0 / PCR1 / PCR2 printed at the end. Capture PCR0 — it's the
# measurement to register on Solana with `add_approved_measurement`.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"
REMOTE_DIR="/home/ec2-user/title-protocol"
EIF_NAME="${EIF_NAME:-title-protocol-tee.eif}"

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"
cd "$REPO_ROOT"

SSH="ssh -i $KEY_PATH -o StrictHostKeyChecking=accept-new ec2-user@$PUBLIC_IP"
SCP="scp -i $KEY_PATH -o StrictHostKeyChecking=accept-new"

echo "==> Packing source tree (excluding target/, legacy/, keys/, .git/)"
TARBALL="$(mktemp -t title-src.XXXXXX.tar.gz)"
tar --exclude='./target' \
    --exclude='./legacy' \
    --exclude='./keys' \
    --exclude='./.git' \
    --exclude='./docs' \
    --exclude='**/target' \
    -czf "$TARBALL" \
    -C "$REPO_ROOT" \
    Cargo.toml Cargo.lock rust-toolchain.toml \
    crates programs sp1-guests \
    docker deploy/aws/docker
du -h "$TARBALL"

echo "==> Uploading source tree to EC2"
$SSH "mkdir -p $REMOTE_DIR/build && rm -rf $REMOTE_DIR/build/*"
$SCP "$TARBALL" "ec2-user@$PUBLIC_IP:$REMOTE_DIR/build/src.tar.gz"
rm -f "$TARBALL"

echo "==> Building images on EC2 (this is the slow step on a clean cache)"
cat <<'REMOTE_SCRIPT' | $SSH "REMOTE_DIR=$REMOTE_DIR bash -s"
set -euo pipefail
cd "$REMOTE_DIR/build"
tar -xzf src.tar.gz

build_image() {
  local tag="$1" dockerfile="$2"
  echo "  ----"
  echo "  Building $tag"
  echo "  Dockerfile: $dockerfile"
  sudo docker build --file "$dockerfile" --tag "$tag" .
}

build_image "title-protocol-tee-nitro:latest" "deploy/aws/docker/tee-nitro.Dockerfile"
build_image "title-protocol-proxy:latest"     "deploy/aws/docker/title-proxy.Dockerfile"
build_image "title-protocol-gateway:latest"   "docker/gateway.Dockerfile"

echo ""
echo "==> Built images on EC2:"
sudo docker image ls --format 'table {{.Repository}}:{{.Tag}}\t{{.Size}}' \
  | grep -E "title-protocol-(tee-nitro|proxy|gateway)"
REMOTE_SCRIPT

echo ""
echo "==> Building EIF (~30 sec)"
$SSH "sudo nitro-cli build-enclave \
        --docker-uri title-protocol-tee-nitro:latest \
        --output-file $REMOTE_DIR/$EIF_NAME"

echo ""
echo "==> Done."
echo ""
echo "PCR values for the resulting EIF (record PCR0 for Solana registration):"
$SSH "sudo nitro-cli describe-eif --eif-path $REMOTE_DIR/$EIF_NAME | jq '.Measurements'"
echo ""
echo "Next: bash deploy/aws/scripts/run-stack.sh"
