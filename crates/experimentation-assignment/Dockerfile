# Multi-stage build for minimal runtime image.
FROM rust:1.88-slim as builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY proto/ proto/
RUN apt-get update && apt-get install -y protobuf-compiler cmake
RUN cargo build --release --package experimentation-assignment

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/experimentation-assignment /usr/local/bin/service
# Bootstrap experiment config: main.rs requires CONFIG_PATH to exist
# (default dev/config.json). Baked until M5 StreamConfigUpdates (ADR-031)
# replaces file-based config in deployment.
COPY dev/config.json /app/config.json
ENV CONFIG_PATH=/app/config.json
EXPOSE 50051
CMD ["service"]
