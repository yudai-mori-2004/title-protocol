#!/bin/bash
# Title Protocol — EC2 first-boot provisioning (cloud-init user-data)
#
# Runs once when the instance launches. Installs the bare minimum needed
# to run a TEE node: Docker (to load images shipped from the operator's
# machine) and the AWS Nitro Enclaves CLI (to build EIFs and run enclaves).
#
# v0.1.2 deliberately does NOT install Rust, Node, Solana CLI, etc. on
# the instance — TEE binaries are built reproducibly on the operator's
# Linux build host and shipped over as Docker images, so the EC2 box
# stays minimal.

set -euo pipefail
exec > >(tee /var/log/title-protocol-provision.log) 2>&1

echo "=== Title Protocol EC2 provisioning ==="
date

# Base updates
dnf update -y

# Docker (used to load shipped images and to drive `nitro-cli build-enclave`)
dnf install -y docker
systemctl enable --now docker
usermod -aG docker ec2-user

# Nitro Enclaves CLI + dev headers (vsock-proxy ships in the same package)
dnf install -y aws-nitro-enclaves-cli aws-nitro-enclaves-cli-devel

# Allocate Enclave resources. memory_mib / cpu_count are passed in from
# Terraform variables so re-tuning is a `terraform apply` away.
cat > /etc/nitro_enclaves/allocator.yaml <<EOF
---
memory_mib: ${enclave_memory_mib}
cpu_count: ${enclave_cpu_count}
EOF

systemctl enable --now nitro-enclaves-allocator
systemctl restart nitro-enclaves-allocator
usermod -aG ne ec2-user

# socat bridges the host TCP port (3000-gateway side) to the Enclave's
# vsock port — this is the inbound counterpart of title-proxy.
dnf install -y socat

# Small convenience utilities for operators logging in
dnf install -y jq tmux

# Marker so scripts can detect that provisioning completed
touch /var/lib/title-protocol-provisioned

echo "=== provisioning complete at $(date) ==="
