# Title Protocol TEE — AWS Nitro Enclave build
# Spec §5.2, §5.4
#
# Same multi-stage layout as docker/tee-mock.Dockerfile, but:
#   - builds with `--no-default-features --features vendor-aws` so the
#     mock runtime is never compiled in
#   - targets linux/amd64 explicitly (Nitro Enclaves require x86_64)
#   - bakes no entrypoint env vars: `TEE_RUNTIME=nitro` is set by the
#     EC2 launch script so a misconfigured run cannot fall back to mock

FROM --platform=linux/amd64 rust:1.93-bookworm AS builder

WORKDIR /build

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

# Stub sources so `cargo build` can resolve + fetch deps before real source.
# Every workspace member needs a manifest + stub, even ones this image
# doesn't actually build.
RUN mkdir -p crates/attestation/src && echo "" > crates/attestation/src/lib.rs \
 && mkdir -p crates/attestation-aws-nitro/src && echo "" > crates/attestation-aws-nitro/src/lib.rs \
 && mkdir -p crates/core/src && echo "" > crates/core/src/lib.rs \
 && mkdir -p crates/crypto/src && echo "" > crates/crypto/src/lib.rs \
 && mkdir -p crates/tee/src && echo "fn main() {}" > crates/tee/src/main.rs && echo "" > crates/tee/src/lib.rs \
 && mkdir -p crates/gateway/src && echo "fn main() {}" > crates/gateway/src/main.rs && echo "" > crates/gateway/src/lib.rs \
 && mkdir -p crates/proxy/src && echo "fn main() {}" > crates/proxy/src/main.rs \
 && mkdir -p crates/solana/src && echo "" > crates/solana/src/lib.rs

# Warm dep cache (no-op if all deps are unchanged across builds)
RUN cargo build --release --bin title-tee \
      --no-default-features \
      --features title-tee/vendor-aws

# Real source
COPY crates/ crates/
RUN find crates -name "*.rs" -exec touch {} + \
 && cargo build --release --bin title-tee \
      --no-default-features \
      --features title-tee/vendor-aws

# --- Runtime stage ---
FROM --platform=linux/amd64 debian:bookworm-slim

# CA certificates for TLS to external upstreams (terminated in title-proxy
# on the host, but the trust store lives in the enclave for header checks).
# socat + iproute2 provide the vsock<->TCP inbound bridge.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    socat \
    iproute2 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee
COPY deploy/aws/docker/tee-entrypoint.sh /usr/local/bin/tee-entrypoint.sh
RUN chmod +x /usr/local/bin/tee-entrypoint.sh

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
