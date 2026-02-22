#!/bin/bash
# Title Protocol EC2 初期セットアップ (user-data)
#
# Amazon Linux 2023 上で実行される。
# Docker, nitro-cli, Node.js をインストールし、
# title-protocol リポジトリをクローンできる状態にする。

set -euo pipefail
exec > >(tee /var/log/title-setup.log) 2>&1

echo "=== Title Protocol EC2 初期セットアップ ==="

# --- パッケージ更新 ---
dnf update -y

# --- Docker ---
dnf install -y docker
systemctl enable docker
systemctl start docker
usermod -aG docker ec2-user

# Docker Compose plugin
mkdir -p /usr/local/lib/docker/cli-plugins
COMPOSE_VERSION="v2.27.0"
curl -SL "https://github.com/docker/compose/releases/download/$${COMPOSE_VERSION}/docker-compose-linux-x86_64" \
  -o /usr/local/lib/docker/cli-plugins/docker-compose
chmod +x /usr/local/lib/docker/cli-plugins/docker-compose

# --- Nitro Enclaves ---
dnf install -y aws-nitro-enclaves-cli aws-nitro-enclaves-cli-devel
systemctl enable nitro-enclaves-allocator
systemctl start nitro-enclaves-allocator

# Enclave用リソース設定
cat > /etc/nitro_enclaves/allocator.yaml <<EOF
---
memory_mib: ${enclave_memory_mib}
cpu_count: ${enclave_cpu_count}
EOF

systemctl restart nitro-enclaves-allocator
usermod -aG ne ec2-user

# --- Node.js 20 (init-config.mjs用) ---
dnf install -y nodejs20

# --- Solana CLI ---
su - ec2-user -c 'sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"'

# --- Git ---
dnf install -y git

# --- ビルド依存 (Cコンパイラ, OpenSSL, pkg-config, socat) ---
dnf install -y gcc gcc-c++ openssl-devel pkg-config socat

# --- Rust + wasm32ターゲット (WASMモジュールビルド + Dockerビルド用) ---
su - ec2-user -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
su - ec2-user -c 'source ~/.cargo/env && rustup target add wasm32-unknown-unknown'
# EC2直接ビルド時にシステムOpenSSLを使用
echo 'export OPENSSL_NO_VENDOR=1' >> /home/ec2-user/.bashrc

# --- 作業ディレクトリ ---
mkdir -p /home/ec2-user/title-protocol
chown ec2-user:ec2-user /home/ec2-user/title-protocol

echo ""
echo "=== 初期セットアップ完了 ==="
echo "次のステップ:"
echo "  1. ssh ec2-user@<public-ip>"
echo "  2. cd title-protocol && git clone ..."
echo "  3. cp .env.example .env && vim .env"
echo "  4. ./deploy/aws/setup-ec2.sh"
