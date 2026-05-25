# syntax=docker/dockerfile:1
# Title Protocol TEE — AWS Nitro Enclave build
# Spec §5.2, §5.4 — Reproducible Build
#
# Same multi-stage layout as docker/tee-mock.Dockerfile, but:
#   - builds with `--no-default-features --features vendor-aws` so the
#     mock runtime is never compiled in
#   - targets linux/amd64 explicitly (Nitro Enclaves require x86_64)
#   - bakes no entrypoint env vars: `TEE_RUNTIME=nitro` is set by the
#     EC2 launch script so a misconfigured run cannot fall back to mock
#
# Reproducibility: images pinned by digest, apt versions pinned,
# SOURCE_DATE_EPOCH as ARG so BuildKit clamps layer tar timestamps.
# To update pins, see docs/v0.1.2/OPERATIONS_JA.md.

# BuildKit reads this ARG to clamp all tar entry mtimes in every layer.
ARG SOURCE_DATE_EPOCH=0

# --- Pin base images by digest for byte-reproducible builds ---
# rust:1.93-bookworm linux/amd64 (2025-05)
FROM --platform=linux/amd64 rust:1.93-bookworm@sha256:1d33950f982ca6411f5e0ee4850be46e03f066f1a9efaeb41922a0e59497c9c2 AS builder

WORKDIR /build

# Reproducibility: strip host paths from panic strings and DWARF info,
# fix embedded timestamps, disable incremental compilation.
ENV SOURCE_DATE_EPOCH=0
ENV CARGO_INCREMENTAL=0
ENV RUSTFLAGS="--remap-path-prefix=/build=/src --remap-path-prefix=/usr/local/cargo=/cargo --remap-path-prefix=/usr/local/rustup=/rustup"

# Manifests + lock first (dependency cache layer)
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/attestation/Cargo.toml crates/attestation/Cargo.toml
COPY crates/attestation-aws-nitro/Cargo.toml crates/attestation-aws-nitro/Cargo.toml
COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/crypto/Cargo.toml crates/crypto/Cargo.toml
COPY crates/tee/Cargo.toml crates/tee/Cargo.toml
COPY crates/gateway/Cargo.toml crates/gateway/Cargo.toml
COPY crates/proxy/Cargo.toml crates/proxy/Cargo.toml
COPY crates/solana/Cargo.toml crates/solana/Cargo.toml
COPY crates/cli/Cargo.toml crates/cli/Cargo.toml

# Stub sources so `cargo build` can resolve + fetch deps before real source.
# Every workspace member needs a manifest + stub, even ones this image
# doesn't actually build.
RUN mkdir -p crates/attestation/src && echo "" > crates/attestation/src/lib.rs \
 && mkdir -p crates/attestation-aws-nitro/src && echo "" > crates/attestation-aws-nitro/src/lib.rs \
 && mkdir -p crates/core/src && echo "" > crates/core/src/lib.rs \
 && mkdir -p crates/crypto/src && echo "" > crates/crypto/src/lib.rs \
 && mkdir -p crates/tee/src && echo "fn main() {}" > crates/tee/src/main.rs && echo "" > crates/tee/src/lib.rs \
 && mkdir -p crates/gateway/src && echo "fn main() {}" > crates/gateway/src/main.rs && echo "" > crates/gateway/src/lib.rs \
 && mkdir -p crates/proxy/src && echo "fn main() {}" > crates/proxy/src/main.rs && echo "" > crates/proxy/src/lib.rs \
 && mkdir -p crates/solana/src && echo "" > crates/solana/src/lib.rs \
 && mkdir -p crates/cli/src && echo "fn main() {}" > crates/cli/src/main.rs

# Warm dep cache (no-op if all deps are unchanged across builds)
RUN cargo build --release --locked --bin title-tee \
      --no-default-features \
      --features title-tee/vendor-aws

# Real source
COPY crates/ crates/
RUN find crates -name "*.rs" -exec touch {} + \
 && cargo build --release --locked --bin title-tee \
      --no-default-features \
      --features title-tee/vendor-aws

# --- Runtime stage ---
# debian:bookworm-slim linux/amd64 (2025-05)
FROM --platform=linux/amd64 debian:bookworm-slim@sha256:b29f74a267526ae6ea104eed6c46133b0ca70ce812525df8cd5817698f0a624a

# CA certificates for TLS to external upstreams (terminated in title-proxy
# on the host, but the trust store lives in the enclave for header checks).
# socat + iproute2 provide the vsock<->TCP inbound bridge.
# Versions pinned for reproducibility (Debian bookworm).
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates=20230311+deb12u1 \
    socat=1.7.4.4-2 \
    iproute2=6.1.0-3 \
    && rm -rf /var/lib/apt/lists/* \
    && rm -f /var/cache/ldconfig/aux-cache \
    && rm -rf /var/log/dpkg.log /var/log/apt /var/log/alternatives.log \
    && find /etc /var /usr -xdev -newermt '@0' \
         -exec touch -h -d '@0' {} + 2>/dev/null || true

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee
COPY deploy/aws/docker/tee-entrypoint.sh /usr/local/bin/tee-entrypoint.sh
RUN chmod +x /usr/local/bin/tee-entrypoint.sh \
    && touch -h -d '@0' /usr/local/bin/title-tee /usr/local/bin/tee-entrypoint.sh

# Production defaults for Nitro:
#   TEE_RUNTIME=nitro       — pick the AWS Nitro runtime explicitly. Without
#                             this, the binary refuses to start (see main.rs
#                             selection logic) because `runtime-mock` was not
#                             compiled in.
#   PROXY_ADDR=vsock://3:8000
#                           — all outbound HTTPS goes via the title-proxy
#                             instance running on the parent EC2 host (CID 3
#                             is the conventional parent-host vsock CID).
ENV TEE_RUNTIME=nitro
ENV PROXY_ADDR=vsock://3:8000

ENTRYPOINT ["/usr/local/bin/tee-entrypoint.sh"]
