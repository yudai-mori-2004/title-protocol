#!/usr/bin/env bash
# Title Protocol — Install build deps + swap on a freshly-launched prover EC2.
#
# Designed to be SSH'd over to the prover by prover-run.sh; can also be
# scp'd up and run directly. Idempotent.
#
# Usage (on the prover):
#   bash prover-setup.sh
#
# What it installs:
#   - gcc / openssl-devel / git / pkgconfig / tar / protobuf-devel
#   - Docker (the SP1 Groth16 wrap shells out to a `gnark` container)
#   - Rust (latest stable via rustup)
#   - SP1 toolchain v5.2.4
#   - 16 GiB swap file (Groth16 wrap peaks at ~95 GiB; c5.12xlarge has 96 GiB
#     physical and gets OOM-killed without swap)

set -euo pipefail

echo "==> [1/5] OS packages"
sudo dnf -y install gcc gcc-c++ openssl-devel git pkgconfig tar protobuf-devel >/dev/null

echo "==> [2/5] Docker (SP1 Groth16 wrap shells out to a gnark container)"
sudo dnf -y install docker >/dev/null
sudo systemctl enable --now docker >/dev/null
sudo usermod -aG docker ec2-user
# The new group membership only applies to fresh login sessions; prover-run.sh
# opens a separate SSH session for prove so it picks up the group there.

echo "==> [3/5] Rust toolchain"
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y >/dev/null
fi
# shellcheck source=/dev/null
source "$HOME/.cargo/env"
rustc --version

echo "==> [4/5] SP1 toolchain v5.2.4"
if [[ ! -d "$HOME/.sp1" ]]; then
  curl -L https://sp1.succinct.xyz | bash >/dev/null
fi
"$HOME/.sp1/bin/sp1up" --version v5.2.4 >/dev/null
echo "SP1 toolchain installed"

echo "==> [5/5] 16 GiB swap"
if ! swapon --show | grep -q '/swapfile'; then
  sudo dd if=/dev/zero of=/swapfile bs=1G count=16 status=none
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile >/dev/null
  sudo swapon /swapfile
fi
free -g | head -3

# Make sure github.com is trusted so `git clone` over SSH (with agent
# forwarding) works without an interactive prompt.
mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh"
if ! grep -q '^github.com' "$HOME/.ssh/known_hosts" 2>/dev/null; then
  ssh-keyscan github.com 2>/dev/null >> "$HOME/.ssh/known_hosts"
fi

echo "==> Done"
