#!/bin/sh
# Title Protocol TEE — Enclave entrypoint.
#
# The Nitro Enclave has no network interface; the only outside link is
# vsock. The TEE binary itself speaks plain TCP (axum), so we run a
# socat bridge inside the Enclave that maps vsock requests onto the
# TEE's local TCP listener.
#
# Direction map:
#   * Inbound  (Host -> Enclave): vsock:4000 -> TCP:127.0.0.1:4000 (this socat)
#   * Outbound (Enclave -> Host): TEE -> vsock:3:8000 (direct via proxy_fetcher;
#                                 PROXY_ADDR=vsock://3:8000 is set in the image)

set -eu

# 127.0.0.1 must be up before socat will bind there. The slim debian
# runtime doesn't enable loopback by default; do it explicitly.
ip link set lo up 2>/dev/null || true

# Inbound bridge. `fork` spawns a child per connection so requests run
# concurrently. `reuseaddr` lets the bridge restart cleanly on enclave reboot.
socat VSOCK-LISTEN:4000,fork,reuseaddr TCP:127.0.0.1:4000 &
SOCAT_PID=$!

# socat が即死すると外側の /health probe が 60s かけてようやく気付く構造に
# なっていた。enclave 内でも明示的に PID 生存を確認し、ダメなら enclave を
# 落として nitro-cli describe-enclaves で TERMINATED として可視化させる。
sleep 1
if ! kill -0 "$SOCAT_PID" 2>/dev/null; then
    echo "[tee-entrypoint] socat (PID=$SOCAT_PID) died immediately after start; aborting enclave" >&2
    exit 1
fi

# Hand off to the TEE binary. If title-tee exits, the Enclave shuts down;
# the socat background process goes with it.
exec /usr/local/bin/title-tee
