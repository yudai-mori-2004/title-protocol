# syntax=docker/dockerfile:1
# Title Protocol TEE — AWS Nitro Enclave build
# Spec §5.2, §5.4 — Reproducible Build
#
# Three-stage build:
#   1. builder  — compile the Rust binary (deterministic via codegen-units=1,
#                 lto=fat, strip=symbols in Cargo.toml [profile.release])
#   2. runtime  — install OS deps, copy binary, clamp all file timestamps
#   3. squash   — FROM scratch + COPY / to flatten into a single layer,
#                 eliminating Docker whiteout entries whose timestamps are
#                 non-deterministic (moby/moby#50063, unfixed as of 2026-05)
#
# Reproducibility guarantees:
#   - Base images pinned by SHA256 digest
#   - apt package versions pinned
#   - All file mtimes clamped to epoch 0
#   - No whiteout entries (squash stage)
#   - Rust binary is byte-identical across builds
#
# To update pins, see docs/v0.1.2/OPERATIONS_JA.md.

# --- Stage 1: Builder ---
# rust:1.93-bookworm linux/amd64 (2025-05)
FROM --platform=linux/amd64 rust:1.93-bookworm@sha256:1d33950f982ca6411f5e0ee4850be46e03f066f1a9efaeb41922a0e59497c9c2 AS builder

WORKDIR /build

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

RUN mkdir -p crates/attestation/src && echo "" > crates/attestation/src/lib.rs \
 && mkdir -p crates/attestation-aws-nitro/src && echo "" > crates/attestation-aws-nitro/src/lib.rs \
 && mkdir -p crates/core/src && echo "" > crates/core/src/lib.rs \
 && mkdir -p crates/crypto/src && echo "" > crates/crypto/src/lib.rs \
 && mkdir -p crates/tee/src && echo "fn main() {}" > crates/tee/src/main.rs && echo "" > crates/tee/src/lib.rs \
 && mkdir -p crates/gateway/src && echo "fn main() {}" > crates/gateway/src/main.rs && echo "" > crates/gateway/src/lib.rs \
 && mkdir -p crates/proxy/src && echo "fn main() {}" > crates/proxy/src/main.rs && echo "" > crates/proxy/src/lib.rs \
 && mkdir -p crates/solana/src && echo "" > crates/solana/src/lib.rs \
 && mkdir -p crates/cli/src && echo "fn main() {}" > crates/cli/src/main.rs

RUN cargo build --release --locked --bin title-tee \
      --no-default-features \
      --features title-tee/vendor-aws

COPY crates/ crates/
RUN find crates -name "*.rs" -exec touch {} + \
 && cargo build --release --locked --bin title-tee \
      --no-default-features \
      --features title-tee/vendor-aws

# --- Stage 2: Runtime (intermediate, will be squashed) ---
FROM --platform=linux/amd64 debian:bookworm-slim@sha256:b29f74a267526ae6ea104eed6c46133b0ca70ce812525df8cd5817698f0a624a AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates=20230311+deb12u1 \
    socat=1.7.4.4-2 \
    iproute2=6.1.0-3 \
    && rm -rf /var/lib/apt/lists/* \
    && rm -f /var/cache/ldconfig/aux-cache \
    && rm -rf /var/log/dpkg.log /var/log/apt /var/log/alternatives.log

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee
COPY deploy/aws/docker/tee-entrypoint.sh /usr/local/bin/tee-entrypoint.sh
RUN chmod +x /usr/local/bin/tee-entrypoint.sh

# Clamp all file timestamps to epoch 0 for reproducibility.
# This runs last so it catches everything apt and COPY touched.
RUN find / -newermt '@0' ! -path '/proc/*' ! -path '/sys/*' \
      -exec touch -h -d '@0' {} + 2>/dev/null || true

# --- Stage 3: Squash (eliminates whiteout entries) ---
# Docker whiteout files (.wh.*) carry non-deterministic timestamps
# that cannot be controlled from inside a RUN command. Copying the
# entire filesystem into a FROM scratch image produces a single flat
# layer with no whiteouts. For Nitro Enclaves this has zero downside
# since nitro-cli build-enclave flattens the image anyway.
FROM scratch
COPY --from=runtime / /
ENV TEE_RUNTIME=nitro
ENV PROXY_ADDR=vsock://3:8000
ENTRYPOINT ["/usr/local/bin/tee-entrypoint.sh"]
