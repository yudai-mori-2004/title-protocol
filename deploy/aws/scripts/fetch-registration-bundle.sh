#!/usr/bin/env bash
# After the enclave is up in release mode, pull the three pieces needed to
# proceed with on-chain registration:
#
#   1. PCR0          — capture from `nitro-cli describe-enclaves`
#   2. solana_pubkey — from GET /solana-keys
#   3. registration  — from GET /solana-keys (base64), written to a file
#      attestation
#
# Outputs land under `deploy/aws/build/registration/` so the SP1 prover
# script and the on-chain register_key step can pick them up.
#
# Refuses to run if the enclave is in DEBUG_MODE — Attestation Documents
# produced in debug mode have zeroed PCRs and are useless on chain.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TERRAFORM_DIR="$REPO_ROOT/deploy/aws/terraform"
OUT_DIR="$REPO_ROOT/deploy/aws/build/registration"

cd "$TERRAFORM_DIR"
PUBLIC_IP="$(terraform output -raw public_ip)"
KEY_PATH="$REPO_ROOT/$(terraform output -raw key_path | sed "s|^.*/keys/|deploy/aws/keys/|")"
cd "$REPO_ROOT"

mkdir -p "$OUT_DIR"
SSH="ssh -i $KEY_PATH -o StrictHostKeyChecking=accept-new ec2-user@$PUBLIC_IP"

echo "==> Verifying enclave is in release mode"
FLAGS=$($SSH 'sudo nitro-cli describe-enclaves | jq -r ".[0].Flags // \"NONE\""')
if [[ "$FLAGS" == *"DEBUG_MODE"* ]]; then
  echo "ERROR: enclave is running in DEBUG_MODE (Flags=$FLAGS)."
  echo "       Attestations from a debug-mode enclave have zeroed PCRs."
  echo "       Restart with: bash deploy/aws/scripts/run-stack.sh"
  exit 1
fi
echo "    Flags: $FLAGS"

echo "==> Capturing measurements"
$SSH 'sudo nitro-cli describe-enclaves | jq ".[0].Measurements"' \
  | tee "$OUT_DIR/measurements.json"
PCR0=$(jq -r '.PCR0' "$OUT_DIR/measurements.json")
echo "$PCR0" > "$OUT_DIR/pcr0.hex"

echo "==> Fetching /solana-keys"
RESPONSE=$(curl -sf "http://$PUBLIC_IP:3000/solana-keys")
echo "$RESPONSE" | jq > "$OUT_DIR/solana-keys.json"

SOLANA_PUBKEY=$(echo "$RESPONSE" | jq -r '.solana_pubkey')
ATT_B64=$(echo "$RESPONSE" | jq -r '.registration_attestation_b64')

if [[ -z "$ATT_B64" || "$ATT_B64" == "null" ]]; then
  echo "ERROR: registration_attestation_b64 is empty/null."
  echo "       The TEE binary is missing the registration-attestation API."
  echo "       Rebuild: bash deploy/aws/scripts/build-on-ec2.sh"
  exit 1
fi

echo "$SOLANA_PUBKEY" > "$OUT_DIR/solana_pubkey.txt"
echo "$ATT_B64" | base64 -d > "$OUT_DIR/attestation.bin"

ATT_BYTES=$(wc -c < "$OUT_DIR/attestation.bin")

echo ""
echo "==> Registration bundle ready:"
echo "    PCR0           : $PCR0"
echo "    solana_pubkey  : $SOLANA_PUBKEY"
echo "    attestation    : $OUT_DIR/attestation.bin ($ATT_BYTES bytes)"
echo ""
echo "Next steps:"
echo "  1. Admin: add_approved_measurement($PCR0)  on devnet"
echo "  2. Generate Groth16 proof from attestation.bin (~90 min on CPU)"
echo "  3. Submit register_key with proof + public_values"
