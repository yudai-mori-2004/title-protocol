#!/usr/bin/env bash
# Title Protocol — EC2 host provisioning from zero.
# Spec §5.4
#
# Run ON the EC2 instance itself (Amazon Linux 2023). Installs everything
# needed to build and run a TEE node: Docker, Nitro Enclaves CLI, socat,
# git, and allocates hugepages for the Enclave.
#
# Idempotent: safe to re-run after an interrupted first attempt.
#
# Usage (after SSH into a fresh Amazon Linux 2023 instance):
#   sudo bash deploy/aws/scripts/setup-host.sh
#
# After this completes, log out and back in (for docker group membership),
# then proceed with: bash deploy/aws/scripts/build.sh

set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "ERROR: run as root (sudo bash $0)" >&2
  exit 1
fi

ENCLAVE_MEM_MIB="${ENCLAVE_MEM_MIB:-4096}"
ENCLAVE_CPU_COUNT="${ENCLAVE_CPU_COUNT:-2}"

echo "=== Title Protocol EC2 provisioning ==="
echo "    $(date)"
echo "    memory: ${ENCLAVE_MEM_MIB} MiB, cpus: ${ENCLAVE_CPU_COUNT}"

echo "==> System updates"
dnf update -y

echo "==> Installing Docker"
dnf install -y docker
systemctl enable --now docker
usermod -aG docker ec2-user

echo "==> Installing Nitro Enclaves CLI"
dnf install -y aws-nitro-enclaves-cli aws-nitro-enclaves-cli-devel

echo "==> Configuring Enclave allocator"
cat > /etc/nitro_enclaves/allocator.yaml <<EOF
---
memory_mib: ${ENCLAVE_MEM_MIB}
cpu_count: ${ENCLAVE_CPU_COUNT}
EOF

systemctl enable --now nitro-enclaves-allocator
systemctl restart nitro-enclaves-allocator
usermod -aG ne ec2-user

echo "==> Installing socat, git, jq, tmux"
dnf install -y socat git jq tmux

echo "==> Verifying installations"
docker --version
nitro-cli --version
socat -V | head -2
git --version

touch /var/lib/title-protocol-provisioned

echo ""
echo "=== Provisioning complete at $(date) ==="
echo ""
echo "IMPORTANT: Log out and back in for group membership (docker, ne)."
echo ""
echo "Next steps:"
echo "  1. exit"
echo "  2. ssh back in"
echo "  3. git clone <repo-url>"
echo "  4. cd title-protocol"
echo "  5. bash deploy/aws/scripts/build.sh"
