# Title Protocol TEE Mock Dockerfile
#
# MockRuntimeを使用してTEEハードウェアなしでノードを動作させる。

# --- ビルドステージ ---
FROM debian:bookworm-slim AS builder

RUN apt-get update && apt-get install -y \
    gcc \
    g++ \
    libssl-dev \
    make \
    pkg-config \
    curl \
    && rm -rf /var/lib/apt/lists/*

ENV OPENSSL_NO_VENDOR=1

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin title-tee

# --- 実行ステージ ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee

ENV MOCK_MODE=true
EXPOSE 4000

ENTRYPOINT ["/usr/local/bin/title-tee"]
