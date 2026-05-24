#!/usr/bin/env bash
# Stop everything on the EC2 host (containers + enclave + bridge).
# Leaves the EC2 instance itself running — use `terraform destroy` for that.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"

ssh -i "$KEY_PATH" -o StrictHostKeyChecking=accept-new ec2-user@"$PUBLIC_IP" bash <<'REMOTE'
set -euo pipefail
sudo docker rm -f title-gateway 2>/dev/null || true
sudo docker rm -f title-proxy 2>/dev/null || true
sudo pkill -f "socat TCP-LISTEN:4000" || true
sudo nitro-cli terminate-enclave --all || true
echo "stack stopped."
REMOTE
