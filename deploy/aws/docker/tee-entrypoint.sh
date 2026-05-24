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

# Hand off to the TEE binary. If title-tee exits, the Enclave shuts down;
# the socat background process goes with it.
exec /usr/local/bin/title-tee
