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
# Verify the fetched binary against a per-arch pinned SHA256. Guards against
# a tampered release asset silently landing as the HEALTHCHECK executable.
RUN set -eux; \
    case "${TARGETARCH}" in \
      amd64) SHA=7e564681110ee4563637457b91e42f62f96b79618a835bb05ae2305acdcc3db0 ;; \
      arm64) SHA=3759148e22a494149a4abae269adee0d20c428b966683426e2319f9047da521d ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" && exit 1 ;; \
    esac; \
    echo "${SHA}  /bin/grpc_health_probe" | sha256sum -c -
COPY --from=builder /build/target/release/experimentation-flags /usr/local/bin/service
EXPOSE 50057
CMD ["service"]
