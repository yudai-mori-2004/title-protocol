FROM amazonlinux:2023 AS builder

RUN dnf install -y gcc gcc-c++ openssl-devel make pkg-config cmake3

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

# workspace全体をコピー
COPY Cargo.toml ./
COPY enclave/ enclave/
COPY proxy/ proxy/
COPY parent/ parent/
COPY wasm-brightness/ wasm-brightness/

# enclaveバイナリのみビルド
ENV OPENSSL_NO_VENDOR=1
RUN cargo build --release --package enclave-app

FROM amazonlinux:2023

# openssl実行時ライブラリ
RUN dnf install -y openssl ca-certificates

COPY --from=builder /app/target/release/enclave-app /usr/local/bin/enclave-app

CMD ["/usr/local/bin/enclave-app"]