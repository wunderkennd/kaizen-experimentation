# Multi-stage build for minimal runtime image.
FROM rust:1.88-slim as builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY proto/ proto/
RUN apt-get update && apt-get install -y protobuf-compiler cmake g++ libssl-dev pkg-config
RUN cargo build --release --package experimentation-policy

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/experimentation-policy /usr/local/bin/service
EXPOSE 50051
CMD ["service"]
