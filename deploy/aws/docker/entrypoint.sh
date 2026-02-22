#!/bin/sh
# Nitro Enclave 内エントリポイント
#
# Enclave内にはTCPネットワークが存在しないため、
# socatでvsock↔TCPのブリッジを構築する。
#
# インバウンド (Gateway → TEE):
#   ホスト側 socat が TCP:4000 → vsock:CID:4000 に転送
#   ↓ vsock で到達
#   Enclave内 socat が vsock:4000 → TCP:localhost:4000 に転送
#   ↓ TCP loopback
#   title-tee が localhost:4000 で待ち受け
#
# アウトバウンド (TEE → 外部API):
#   title-tee が PROXY_ADDR=127.0.0.1:8000 に接続
#   ↓ TCP loopback
#   Enclave内 socat が TCP:8000 → vsock:3(ホスト):8000 に転送
#   ↓ vsock で到達
#   ホスト側 title-proxy が vsock:8000 で待ち受け → 外部HTTPに転送

set -e

# .env読み込み（Dockerfile内にベイク済み）
if [ -f /.env ]; then
  set -a
  . /.env
  set +a
fi

# Enclave固有の上書き（.envの値に関わらず強制）
export TEE_RUNTIME=nitro
export PROXY_ADDR=127.0.0.1:8000
export WASM_DIR=/wasm-modules

# インバウンドブリッジ: vsock port 4000 → TCP localhost:4000
socat VSOCK-LISTEN:4000,fork TCP:127.0.0.1:4000 &

# アウトバウンドブリッジ: TCP localhost:8000 → vsock CID=3(ホスト) port 8000
socat TCP-LISTEN:8000,fork,reuseaddr VSOCK-CONNECT:3:8000 &

# TEE本体起動
exec /usr/local/bin/title-tee
