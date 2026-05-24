#!/usr/bin/env bash
# Ship the three locally-built images to the EC2 instance, then build the
# Enclave Image File (EIF) on the host from the TEE image.
#
# Steps:
#   1. wait for cloud-init to finish provisioning
#   2. docker save tee/proxy/gateway → scp → docker load
#   3. nitro-cli build-enclave from the TEE image → EIF on the host
#   4. print PCR0/PCR1/PCR2 — record PCR0 for on-chain registration
#
# Idempotent: re-running with the same image tags simply re-loads them.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"
EIF_NAME="${EIF_NAME:-title-protocol-tee.eif}"
REMOTE_DIR="/home/ec2-user/title-protocol"
WORK_DIR="${TMPDIR:-/tmp}/title-protocol-ship"

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"
cd "$REPO_ROOT"

if [[ ! -f "$KEY_PATH" ]]; then
  echo "SSH key not found at $KEY_PATH — run terraform apply first." >&2
  exit 1
fi

SSH="ssh -i $KEY_PATH -o StrictHostKeyChecking=accept-new ec2-user@$PUBLIC_IP"
SCP="scp -i $KEY_PATH -o StrictHostKeyChecking=accept-new"

echo "==> Target: ec2-user@$PUBLIC_IP"
echo "==> Waiting for cloud-init to finish (this is a no-op after the first boot)..."
$SSH "until [[ -f /var/lib/title-protocol-provisioned ]]; do echo '  still provisioning...'; sleep 10; done"

$SSH "mkdir -p $REMOTE_DIR"
mkdir -p "$WORK_DIR"

ship() {
  local image="$1"
  local tarball="$WORK_DIR/$(echo "$image" | tr ':/' '_').tar"
  echo ""
  echo "==> Saving + uploading $image"
  docker save "$image" -o "$tarball"
  du -h "$tarball" | awk '{print "    local tarball size:", $1}'
  $SCP "$tarball" "ec2-user@$PUBLIC_IP:$REMOTE_DIR/$(basename "$tarball")"
  $SSH "sudo docker load -i $REMOTE_DIR/$(basename "$tarball") && rm $REMOTE_DIR/$(basename "$tarball")"
  rm "$tarball"
}

ship "title-protocol-tee-nitro:latest"
ship "title-protocol-proxy:latest"
ship "title-protocol-gateway:latest"

echo ""
echo "==> Building EIF on EC2 (30-60s)"
$SSH "sudo nitro-cli build-enclave \
        --docker-uri title-protocol-tee-nitro:latest \
        --output-file $REMOTE_DIR/$EIF_NAME"

echo ""
echo "==> Recording measurements"
$SSH "sudo nitro-cli describe-eif --eif-path $REMOTE_DIR/$EIF_NAME \
        | jq '.Measurements | {PCR0, PCR1, PCR2}'"

echo ""
echo "EIF + images are loaded on the instance."
echo "Next: bash deploy/aws/scripts/run-stack.sh"
