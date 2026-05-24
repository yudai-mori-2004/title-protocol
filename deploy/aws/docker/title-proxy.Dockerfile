# Title Protocol HTTP proxy — runs on the EC2 host alongside the Enclave.
#
# Listens on vsock port 8000 (CID_ANY) and forwards every request to the
# real public network. The Enclave sees a vsock socket; the outside world
# sees a normal HTTPS client coming from the EC2 host.
#
# Built once per release on the operator's Linux build host (or via
# `docker buildx --platform linux/amd64` on Mac). Image is shipped to EC2
# with `docker save | ssh docker load` and started via
# `docker run --network host title-protocol-proxy:latest`.

FROM --platform=linux/amd64 rust:1.93-bookworm AS builder

WORKDIR /build

# Manifests + lock first for dependency caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/attestation/Cargo.toml crates/attestation/Cargo.toml
COPY crates/attestation-aws-nitro/Cargo.toml crates/attestation-aws-nitro/Cargo.toml
COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/crypto/Cargo.toml crates/crypto/Cargo.toml
COPY crates/tee/Cargo.toml crates/tee/Cargo.toml
COPY crates/gateway/Cargo.toml crates/gateway/Cargo.toml
COPY crates/proxy/Cargo.toml crates/proxy/Cargo.toml
COPY crates/solana/Cargo.toml crates/solana/Cargo.toml

RUN mkdir -p crates/attestation/src && echo "" > crates/attestation/src/lib.rs \
 && mkdir -p crates/attestation-aws-nitro/src && echo "" > crates/attestation-aws-nitro/src/lib.rs \
 && mkdir -p crates/core/src && echo "" > crates/core/src/lib.rs \
 && mkdir -p crates/crypto/src && echo "" > crates/crypto/src/lib.rs \
 && mkdir -p crates/tee/src && echo "fn main() {}" > crates/tee/src/main.rs && echo "" > crates/tee/src/lib.rs \
 && mkdir -p crates/gateway/src && echo "fn main() {}" > crates/gateway/src/main.rs && echo "" > crates/gateway/src/lib.rs \
 && mkdir -p crates/proxy/src && echo "fn main() {}" > crates/proxy/src/main.rs \
 && mkdir -p crates/solana/src && echo "" > crates/solana/src/lib.rs

# title-proxy ships with `default = []`; pass --features vendor-aws here
# to pull in the vsock listener. linux/amd64 only.
RUN cargo build --release --bin title-proxy --features vendor-aws 2>&1

COPY crates/ crates/
RUN find crates -name "*.rs" -exec touch {} + \
 && cargo build --release --bin title-proxy --features vendor-aws

# --- Runtime stage ---
FROM --platform=linux/amd64 debian:bookworm-slim

# CA certificates only — the proxy talks HTTPS to arbitrary upstreams.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/title-proxy /usr/local/bin/title-proxy

ENV RUST_LOG=info
ENTRYPOINT ["/usr/local/bin/title-proxy"]
