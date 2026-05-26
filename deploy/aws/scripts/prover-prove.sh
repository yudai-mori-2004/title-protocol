#!/usr/bin/env bash
# Title Protocol — Generate a Groth16 proof on a prepared prover EC2.
#
# Run on the prover AFTER prover-setup.sh. Source can be supplied two ways:
#   1. A source tarball uploaded to /tmp/source.tar.gz (default — no GitHub
#      auth needed, matches the operator's exact working tree)
#   2. A live git clone (set REPO_URL + ensure SSH agent forwarding works)
#
# Outputs land in the same directory as the input:
#   <attestation>.proof.bin           — 260-byte Groth16 proof
#   <attestation>.public_values.bin   — ZKP public values envelope
#   <attestation>.vkey_hash.hex       — sanity-check vkey hash
#
# Usage:
#   bash prover-prove.sh <path/to/attestation.bin>
#
# Env:
#   SOURCE_TARBALL  path to a .tar.gz of the repository (default: /tmp/source.tar.gz)
#   REPO_URL        if set AND SOURCE_TARBALL is missing, git clone this URL instead
#   REPO_DIR        where to extract / clone the repo (default: ~/title-protocol)

set -euo pipefail

ATTESTATION_PATH="${1:?attestation.bin path required}"
SOURCE_TARBALL="${SOURCE_TARBALL:-/tmp/source.tar.gz}"
REPO_URL="${REPO_URL:-}"
REPO_DIR="${REPO_DIR:-$HOME/title-protocol}"

# shellcheck source=/dev/null
source "$HOME/.cargo/env"

if [[ -d "${REPO_DIR}/.git" || -f "${REPO_DIR}/Cargo.toml" ]]; then
  echo "==> Repo already at ${REPO_DIR}"
elif [[ -f "${SOURCE_TARBALL}" ]]; then
  echo "==> Extracting ${SOURCE_TARBALL} -> ${REPO_DIR}"
  mkdir -p "${REPO_DIR}"
  tar -xzf "${SOURCE_TARBALL}" -C "${REPO_DIR}"
elif [[ -n "${REPO_URL}" ]]; then
  echo "==> Cloning ${REPO_URL} -> ${REPO_DIR}"
  git clone "${REPO_URL}" "${REPO_DIR}"
else
  echo "ERROR: no source available — expected ${SOURCE_TARBALL} or REPO_URL" >&2
  exit 1
fi

HOST_DIR="${REPO_DIR}/sp1-guests/attestation-aws-nitro/host"

echo "==> Placing attestation.bin"
cp "${ATTESTATION_PATH}" "${HOST_DIR}/attestation.bin"
ls -la "${HOST_DIR}/attestation.bin"

echo "==> Building prove + vkey (release, locked)"
(cd "${HOST_DIR}" && cargo build --release --bin prove --bin vkey --locked)

echo "==> vkey_hash:"
"${HOST_DIR}/target/release/vkey"

echo "==> Generating Groth16 proof (~90 min on c5.12xlarge for SP1 v5)"
( cd "${HOST_DIR}" && RUST_LOG=info ./target/release/prove ./attestation.bin )

echo "==> Artifacts:"
ls -la "${HOST_DIR}"/attestation.bin.*
