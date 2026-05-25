#!/usr/bin/env bash
# Title Protocol — Capture registration bundle from a running Enclave.
# Spec §6
#
# Run ON the EC2 instance. Collects the three pieces needed for on-chain
# key registration:
#   1. PCR0          — from nitro-cli describe-enclaves
#   2. solana_pubkey — from GET /solana-keys
#   3. attestation   — from GET /solana-keys (base64 → binary file)
#
# Output lands under deploy/aws/build/registration/.
#
# Refuses to run if the Enclave is in DEBUG_MODE — debug Attestation
# Documents have zeroed PCRs and are useless for on-chain registration.
#
# Usage:
#   bash deploy/aws/scripts/fetch-registration-bundle.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT_DIR="$REPO_ROOT/deploy/aws/build/registration"
mkdir -p "$OUT_DIR"

echo "==> Verifying Enclave is in release mode"
FLAGS=$(sudo nitro-cli describe-enclaves | jq -r '.[0].Flags // "NONE"')
if [[ "$FLAGS" == *"DEBUG_MODE"* ]]; then
  echo "ERROR: Enclave is running in DEBUG_MODE (Flags=$FLAGS)." >&2
  echo "       Attestations from a debug-mode Enclave have zeroed PCRs." >&2
  echo "       Restart with: bash deploy/aws/scripts/run.sh" >&2
  exit 1
fi
echo "    Flags: $FLAGS"

echo "==> Capturing measurements"
sudo nitro-cli describe-enclaves | jq '.[0].Measurements' \
  | tee "$OUT_DIR/measurements.json"
PCR0=$(jq -r '.PCR0' "$OUT_DIR/measurements.json")
echo "$PCR0" > "$OUT_DIR/pcr0.hex"

echo "==> Fetching /solana-keys"
RESPONSE=$(curl -sf http://127.0.0.1:3000/solana-keys)
echo "$RESPONSE" | jq > "$OUT_DIR/solana-keys.json"

SOLANA_PUBKEY=$(echo "$RESPONSE" | jq -r '.solana_pubkey')
ATT_B64=$(echo "$RESPONSE" | jq -r '.registration_attestation_b64')

if [[ -z "$ATT_B64" || "$ATT_B64" == "null" ]]; then
  echo "ERROR: registration_attestation_b64 is empty/null." >&2
  echo "       The TEE binary may be missing the registration-attestation API." >&2
  echo "       Rebuild: bash deploy/aws/scripts/build.sh" >&2
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
echo "  1. Admin: add_approved_measurement(PCR0) on devnet"
echo "  2. Generate Groth16 proof from attestation.bin"
echo "  3. Submit register_key with proof + public_values"
