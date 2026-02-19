# Dockerfile
FROM amazonlinux:2023 AS builder

RUN dnf install -y gcc openssl-devel
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release

FROM amazonlinux:2023

COPY --from=builder /app/target/release/hello-enclave /usr/local/bin/hello-enclave

CMD ["/usr/local/bin/hello-enclave"]