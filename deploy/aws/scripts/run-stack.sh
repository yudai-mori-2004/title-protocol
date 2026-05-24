#!/usr/bin/env bash
# Start the full TEE stack on EC2:
#   - title-proxy   (vsock:8000, container, --network host)
#   - Nitro Enclave (vsock:4000, runs the TEE image)
#   - socat bridge  (host TCP 127.0.0.1:4000 -> vsock:<enclave_cid>:4000)
#   - title-gateway (host port 3000, container, --network host)
#
# Idempotent: stops any running stack before starting a fresh one.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"
EIF_NAME="${EIF_NAME:-title-protocol-tee.eif}"
REMOTE_DIR="/home/ec2-user/title-protocol"
ENCLAVE_MEM_MIB="${ENCLAVE_MEM_MIB:-2048}"
ENCLAVE_CPU_COUNT="${ENCLAVE_CPU_COUNT:-2}"
# Default is release mode. Set ENCLAVE_DEBUG=1 to attach `nitro-cli console`
# during development — but note that debug mode causes the NSM to emit
# all-zero PCR values in its Attestation Documents, so any attestation
# captured under ENCLAVE_DEBUG=1 is useless for on-chain registration.
ENCLAVE_DEBUG="${ENCLAVE_DEBUG:-0}"
if [[ "$ENCLAVE_DEBUG" == "1" ]]; then
  DEBUG_FLAG="--debug-mode"
  echo "WARNING: ENCLAVE_DEBUG=1 — Attestation Documents from this enclave will have zeroed PCRs."
else
  DEBUG_FLAG=""
fi

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"
cd "$REPO_ROOT"

SSH="ssh -i $KEY_PATH -o StrictHostKeyChecking=accept-new ec2-user@$PUBLIC_IP"

cat <<REMOTE_SCRIPT | $SSH "REMOTE_DIR=$REMOTE_DIR EIF_NAME=$EIF_NAME ENCLAVE_MEM_MIB=$ENCLAVE_MEM_MIB ENCLAVE_CPU_COUNT=$ENCLAVE_CPU_COUNT DEBUG_FLAG='$DEBUG_FLAG' API_KEYS='${API_KEYS:-}' bash -s"
set -euo pipefail
cd "\$REMOTE_DIR"

echo "==> Stopping any previous stack"
sudo docker rm -f title-gateway 2>/dev/null || true
sudo docker rm -f title-proxy 2>/dev/null || true
sudo pkill -f "socat TCP-LISTEN:4000" || true
sudo nitro-cli terminate-enclave --all || true
sleep 2

echo "==> Starting title-proxy (host network, vsock:8000)"
# AF_VSOCK bind requires /dev/vsock access; --device exposes it without
# granting the full --privileged surface (CAP_SYS_ADMIN, seccomp off, etc.).
sudo docker run -d --name title-proxy \
  --restart unless-stopped \
  --network host \
  --device /dev/vsock \
  title-protocol-proxy:latest

if [[ -n "\$DEBUG_FLAG" ]]; then
  MODE_LABEL="debug"
else
  MODE_LABEL="release"
fi
echo "==> Launching Nitro Enclave (mode=\$MODE_LABEL, mem=\${ENCLAVE_MEM_MIB} MiB, cpus=\${ENCLAVE_CPU_COUNT})"
sudo nitro-cli run-enclave \
  --eif-path "\$REMOTE_DIR/\$EIF_NAME" \
  --memory "\$ENCLAVE_MEM_MIB" \
  --cpu-count "\$ENCLAVE_CPU_COUNT" \
  \$DEBUG_FLAG

ENCLAVE_CID=\$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveCID')
echo "    enclave CID: \$ENCLAVE_CID"

echo "==> Starting host -> Enclave bridge (TCP 127.0.0.1:4000 -> vsock:\$ENCLAVE_CID:4000)"
nohup sudo socat TCP-LISTEN:4000,reuseaddr,fork VSOCK-CONNECT:\$ENCLAVE_CID:4000 \
  > "\$REMOTE_DIR/socat.log" 2>&1 &

echo "==> Waiting for TEE HTTP to come up..."
for i in {1..60}; do
  if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
    echo "    TEE ready (\${i}s)"
    break
  fi
  sleep 1
done

echo "==> Starting title-gateway (host network, port 3000)"
sudo docker run -d --name title-gateway \
  --restart unless-stopped \
  --network host \
  -e TEE_ENDPOINT=http://127.0.0.1:4000 \
  -e API_KEYS="\$API_KEYS" \
  title-protocol-gateway:latest

sleep 2
echo ""
echo "==> Status"
sudo nitro-cli describe-enclaves | jq '.[0] | {EnclaveCID, State, CPUCount, MemoryMiB}'
sudo docker ps --format 'table {{.Names}}\t{{.Status}}' \
  --filter name=title-proxy --filter name=title-gateway
REMOTE_SCRIPT

echo ""
echo "==> Stack should be live. Sanity check:"
echo "    curl http://$PUBLIC_IP:3000/health"
echo "    bash deploy/aws/scripts/tee-console.sh"
