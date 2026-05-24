#!/usr/bin/env bash
# Open an SSH shell to the EC2 instance.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"

exec ssh -i "$KEY_PATH" -o StrictHostKeyChecking=accept-new ec2-user@"$PUBLIC_IP" "$@"
