#!/usr/bin/env bash
# Attach to the running Nitro Enclave's console (stdout/stderr).
# Requires the enclave to have been started with `--debug-mode`.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"

ssh -i "$KEY_PATH" -o StrictHostKeyChecking=accept-new -t ec2-user@"$PUBLIC_IP" \
  "sudo nitro-cli console --enclave-id \"\$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')\""
