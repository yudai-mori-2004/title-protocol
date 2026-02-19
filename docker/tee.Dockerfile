# Title Protocol TEE Dockerfile (本番Enclave用)
#
# AWS Nitro Enclaves向けのマルチステージビルド。
# nitro-cli build-enclave でEIFに変換される。

# --- ビルドステージ ---
FROM amazonlinux:2023 AS builder

RUN dnf install -y \
    gcc \
    gcc-c++ \
    openssl-devel \
    perl-FindBin \
    perl-File-Compare \
    perl-IPC-Cmd \
    perl-File-Copy \
    make \
    && dnf clean all

# Rustツールチェーンのインストール
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin title-tee

# --- 実行ステージ ---
FROM amazonlinux:2023

RUN dnf install -y \
    openssl \
    ca-certificates \
    && dnf clean all

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee

ENTRYPOINT ["/usr/local/bin/title-tee"]
