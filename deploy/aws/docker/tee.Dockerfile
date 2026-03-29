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
    make \
    pkg-config \
    && dnf clean all

ENV OPENSSL_NO_VENDOR=1

# Rustツールチェーンのインストール
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin title-tee

# --- 実行ステージ ---
FROM amazonlinux:2023

# Static ffmpeg/ffprobe — zero shared library dependencies
COPY --from=mwader/static-ffmpeg:7.1 /ffmpeg /usr/local/bin/ffmpeg
COPY --from=mwader/static-ffmpeg:7.1 /ffprobe /usr/local/bin/ffprobe

# ExifTool — pure Perl, install from official tarball
ENV EXIFTOOL_VERSION=13.50

RUN dnf install -y \
    openssl \
    ca-certificates \
    socat \
    iproute \
    perl \
    make \
    tar \
    gzip \
    && curl -sL https://exiftool.org/Image-ExifTool-${EXIFTOOL_VERSION}.tar.gz | tar xz -C /tmp \
    && cd /tmp/Image-ExifTool-${EXIFTOOL_VERSION} \
    && perl Makefile.PL && make install \
    && cd / && rm -rf /tmp/Image-ExifTool-* \
    && dnf remove -y make \
    && dnf clean all

COPY --from=builder /build/target/release/title-tee /usr/local/bin/title-tee
COPY deploy/aws/docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# .env（gitignore済み）からEnclave内の環境変数をベイク
COPY .env /.env

# WASMモジュール（ホスト上で事前ビルド済みのものをコピー）
COPY wasm-modules/ /wasm-modules/

ENTRYPOINT ["/entrypoint.sh"]
