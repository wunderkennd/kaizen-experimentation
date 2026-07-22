# Multi-stage build for minimal runtime image.
FROM rust:1.88-slim as builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY proto/ proto/
RUN apt-get update && apt-get install -y protobuf-compiler cmake g++ libssl-dev libcurl4-openssl-dev pkg-config
RUN cargo build --release --package experimentation-flags

FROM debian:bookworm-slim
ARG TARGETARCH
ARG GRPC_HEALTH_PROBE_VERSION=v0.4.24
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
# grpc_health_probe: bundled for ECS/Cloud Run container HEALTHCHECK CMD.
# TARGETARCH resolves to amd64|arm64 during buildx multi-arch push (#755).
ADD --chmod=755 https://github.com/grpc-ecosystem/grpc-health-probe/releases/download/${GRPC_HEALTH_PROBE_VERSION}/grpc_health_probe-linux-${TARGETARCH} /bin/grpc_health_probe
COPY --from=builder /build/target/release/experimentation-flags /usr/local/bin/service
EXPOSE 50057
CMD ["service"]
