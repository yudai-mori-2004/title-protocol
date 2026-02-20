# Title Protocol Gateway Dockerfile

# --- ビルドステージ ---
FROM amazonlinux:2023 AS builder

RUN dnf install -y \
    gcc \
    gcc-c++ \
    openssl-devel \
    make \
    pkg-config \
    && dnf clean all

ENV OPENSSL_NO_VENDOR=1

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin title-gateway

# --- 実行ステージ ---
FROM amazonlinux:2023

RUN dnf install -y \
    openssl \
    ca-certificates \
    && dnf clean all

COPY --from=builder /build/target/release/title-gateway /usr/local/bin/title-gateway

EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/title-gateway"]
