# Title Protocol TEE (mock) — Multi-stage build
# Spec §5.4 — Reproducible build via Cargo.lock + rust-toolchain.toml

# --- Build stage ---
FROM rust:1.93-bookworm AS builder

WORKDIR /build

# Copy manifests + lock first for dependency caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/attestation/Cargo.toml crates/attestation/Cargo.toml
COPY crates/attestation-aws-nitro/Cargo.toml crates/attestation-aws-nitro/Cargo.toml
COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/crypto/Cargo.toml crates/crypto/Cargo.toml
COPY crates/tee/Cargo.toml crates/tee/Cargo.toml
COPY crates/gateway/Cargo.toml crates/gateway/Cargo.toml
COPY crates/solana/Cargo.toml crates/solana/Cargo.toml

# Stub sources — lets cargo resolve and cache all dependencies
RUN mkdir -p crates/attestation/src && echo "" > crates/attestation/src/lib.rs \
 && mkdir -p crates/attestation-aws-nitro/src && echo "" > crates/attestation-aws-nitro/src/lib.rs \
 && mkdir -p crates/core/src && echo "" > crates/core/src/lib.rs \
 && mkdir -p crates/crypto/src && echo "" > crates/crypto/src/lib.rs \
 && mkdir -p crates/tee/src && echo "fn main() {}" > crates/tee/src/main.rs && echo "" > crates/tee/src/lib.rs \
 && mkdir -p crates/gateway/src && echo "fn main() {}" > crates/gateway/src/main.rs && echo "" > crates/gateway/src/lib.rs \
 && mkdir -p crates/solana/src && echo "" > crates/solana/src/lib.rs

RUN cargo build --release --bin title-tee --features title-tee/runtime-mock 2>&1 || true

# Real source
COPY crates/ crates/
RUN find crates -name "*.rs" -exec touch {} + \
 && cargo build --release --bin title-tee --features title-tee/runtime-mock

# --- Runtime stage ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee

ENV TEE_RUNTIME=mock
EXPOSE 4000

ENTRYPOINT ["/usr/local/bin/title-tee"]
