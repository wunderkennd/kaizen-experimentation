# Multi-stage build for minimal runtime image.
FROM rust:1.80-slim as builder
WORKDIR /build
COPY crates/ crates/
COPY proto/ proto/
RUN apt-get update && apt-get install -y protobuf-compiler cmake
WORKDIR /build/crates
RUN cargo build --release --package experimentation-assignment

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/crates/target/release/experimentation-assignment /usr/local/bin/service
EXPOSE 50051
CMD ["service"]
