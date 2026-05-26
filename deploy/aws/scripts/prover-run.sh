#!/usr/bin/env bash
# Title Protocol — One-shot SP1 proof generation on a fresh EC2.
#
# End-to-end wrapper: launches a prover EC2, uploads attestation.bin,
# installs toolchains, generates the Groth16 proof, downloads artifacts
# into deploy/aws/build/registration/, and terminates the instance.
#
# Usage:
#   bash deploy/aws/scripts/prover-run.sh [<attestation.bin>] [--keep-alive]
#
# Default attestation: deploy/aws/build/registration/attestation.bin
# (the path written by fetch-registration-bundle.sh).
#
# --keep-alive: do NOT terminate the prover at the end (useful when
# debugging or re-running). You can terminate later with:
#   aws ec2 terminate-instances --instance-ids <id>
#
# Total wall clock: ~110 min on c5.12xlarge (~$4 in Tokyo) for SP1 v5.
# (SP1 v6 would be ~30 min / ~$1, but on-chain sp1-solana 0.1.0 only handles
#  v5 wire format — see PCR0_REPRODUCIBILITY_INVESTIGATION.md SP1 version pin)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT_DIR="${REPO_ROOT}/deploy/aws/scripts"
KEY_PATH="${REPO_ROOT}/deploy/aws/keys/title-protocol-devnet.pem"

ATTESTATION="${REPO_ROOT}/deploy/aws/build/registration/attestation.bin"
KEEP_ALIVE=false

for arg in "$@"; do
  case "$arg" in
    --keep-alive) KEEP_ALIVE=true ;;
    -h|--help)
      sed -n '2,/^$/p' "$0" | sed 's/^# *//'
      exit 0 ;;
    *) ATTESTATION="$arg" ;;
  esac
done

if [[ ! -f "${ATTESTATION}" ]]; then
  echo "ERROR: attestation.bin not found at ${ATTESTATION}" >&2
  echo "       Run 'bash deploy/aws/scripts/fetch-registration-bundle.sh' on the TEE node first." >&2
  exit 1
fi

if [[ ! -f "${KEY_PATH}" ]]; then
  echo "ERROR: SSH key not found at ${KEY_PATH}" >&2
  echo "       Expected the Terraform-generated key (deploy/aws/terraform)." >&2
  exit 1
fi

echo "==> Step 1/5: launch prover EC2"
LAUNCH_OUT=$(bash "${SCRIPT_DIR}/prover-launch.sh")
INSTANCE_ID=$(echo "${LAUNCH_OUT}" | sed -n '1p')
PUBLIC_IP=$(echo "${LAUNCH_OUT}" | sed -n '2p')
echo "    instance: ${INSTANCE_ID}  ip: ${PUBLIC_IP}"

ssh_to_prover() {
  ssh -i "${KEY_PATH}" -o StrictHostKeyChecking=no -o ServerAliveInterval=30 \
      ec2-user@"${PUBLIC_IP}" "$@"
}

scp_to_prover() {
  scp -i "${KEY_PATH}" -o StrictHostKeyChecking=no "$@"
}

echo "==> Step 2/5: install toolchains + swap on prover"
scp_to_prover "${SCRIPT_DIR}/prover-setup.sh" "ec2-user@${PUBLIC_IP}:/tmp/prover-setup.sh"
scp_to_prover "${SCRIPT_DIR}/prover-prove.sh" "ec2-user@${PUBLIC_IP}:/tmp/prover-prove.sh"
ssh_to_prover 'bash /tmp/prover-setup.sh'

echo "==> Step 3/5: upload attestation.bin + source tarball"
scp_to_prover "${ATTESTATION}" "ec2-user@${PUBLIC_IP}:/tmp/attestation.bin"

# Bundle the repository as a tarball so the prover does not need GitHub
# access. `git archive HEAD` captures the exact committed tree (uncommitted
# edits are intentionally excluded — prove against what is committed).
TMP_TARBALL="$(mktemp -t prover-source.XXXXXX.tar.gz)"
trap 'rm -f "${TMP_TARBALL}"' EXIT
git -C "${REPO_ROOT}" archive --format=tar.gz HEAD > "${TMP_TARBALL}"
scp_to_prover "${TMP_TARBALL}" "ec2-user@${PUBLIC_IP}:/tmp/source.tar.gz"

echo "==> Step 4/5: build + prove on prover (~90 min for SP1 v5)"
ssh_to_prover 'bash /tmp/prover-prove.sh /tmp/attestation.bin'

echo "==> Step 5/5: download artifacts"
mkdir -p "${REPO_ROOT}/deploy/aws/build/registration"
scp_to_prover \
  "ec2-user@${PUBLIC_IP}:~/title-protocol/sp1-guests/attestation-aws-nitro/host/attestation.bin.*" \
  "${REPO_ROOT}/deploy/aws/build/registration/"

echo ""
echo "=== Proof artifacts ==="
ls -la "${REPO_ROOT}/deploy/aws/build/registration/"attestation.bin.*

if $KEEP_ALIVE; then
  echo ""
  echo "==> Keeping prover alive at ${PUBLIC_IP} (--keep-alive)"
  echo "    Terminate later with:"
  echo "      aws ec2 terminate-instances --instance-ids ${INSTANCE_ID}"
else
  echo ""
  echo "==> Terminating prover ${INSTANCE_ID}"
  aws ec2 terminate-instances --instance-ids "${INSTANCE_ID}" --output text >/dev/null
  echo "    Terminated."
fi

echo ""
echo "==> Next: submit register_key with the proof bundle:"
echo "    title-cli register-key \\"
echo "      --payer keys/admin.json \\"
echo "      --bundle deploy/aws/build/registration"
