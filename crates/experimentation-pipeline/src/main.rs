//! M2 Event Pipeline — gRPC ingestion server.
//!
//! Crash-only design: no SIGTERM handler, no graceful shutdown.
//! On restart, the Bloom filter resets (brief dedup gap accepted per design doc).
//! Kafka idempotent producer ensures no duplicates on the broker side.
//! If buffer file exists from a previous crash, replay events before accepting new ones.

mod buffer;
mod kafka;
mod metrics;
mod service;

use std::net::SocketAddr;
use std::path::PathBuf;

use experimentation_ingest::dedup::{DedupConfig, DedupMetrics, EventDedup};
use experimentation_proto::pipeline::event_ingestion_service_server::EventIngestionServiceServer;
use prometheus::Registry;
use tracing::info;

use crate::buffer::{BufferConfig, DiskBuffer};
use crate::kafka::{EventProducer, KafkaConfig};
use crate::metrics::PipelineMetrics;
use crate::service::IngestionServiceImpl;

/// Default gRPC port for the M2 ingest service. Owned by M2 per `CLAUDE.md`.
/// Must not collide with M1 Assignment (50051).
const DEFAULT_PORT: &str = "50052";

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for #460: the M2 Pipeline default port must not collide
    /// with M1 Assignment, which owns 50051. Per `CLAUDE.md`, the M2 ingest
    /// service owns 50052. The previous default — `"50051"` — meant any
    /// deployment that did not explicitly set `PORT` would race M1 for the
    /// port. Local dev was fine only because `justfile` overrides `PORT=50052`.
    #[test]
    fn default_port_does_not_collide_with_m1_assignment() {
        let port: u16 = DEFAULT_PORT.parse().expect("DEFAULT_PORT must be a valid u16");
        const M1_ASSIGNMENT_PORT: u16 = 50051;
        const M2_INGEST_PORT: u16 = 50052;
        assert_ne!(port, M1_ASSIGNMENT_PORT, "M2 default collides with M1 Assignment");
        assert_eq!(port, M2_INGEST_PORT, "M2 default must be 50052 per CLAUDE.md");
    }
}

/// Serve Prometheus metrics + health check endpoints on a separate HTTP endpoint.
///
/// - `GET /metrics` — Prometheus scrape endpoint
/// - `GET /healthz` — Liveness probe (always 200 if process is running)
/// - `GET /readyz` — Readiness probe (always 200; Kafka producer initializes at startup)
async fn serve_metrics(addr: SocketAddr, registry: Registry) {
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::service::service_fn;
    use hyper::{Request, Response};
    use hyper_util::rt::TokioIo;
    use prometheus::Encoder;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(addr).await.expect("metrics bind failed");
    info!(%addr, "Prometheus metrics + health endpoints listening");

    loop {
        let (stream, _) = listener.accept().await.expect("metrics accept failed");
        let io = TokioIo::new(stream);
        let registry = registry.clone();

        tokio::spawn(async move {
            let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
                let registry = registry.clone();
                async move {
                    match req.uri().path() {
                        "/healthz" => Ok::<_, hyper::Error>(
                            Response::new(Full::new(Bytes::from_static(b"ok"))),
                        ),
                        "/readyz" => Ok::<_, hyper::Error>(
                            Response::new(Full::new(Bytes::from_static(b"ok"))),
                        ),
                        _ => {
                            let encoder = prometheus::TextEncoder::new();
                            let metric_families = registry.gather();
                            let mut buffer = Vec::new();
                            encoder.encode(&metric_families, &mut buffer).unwrap();
                            Ok::<_, hyper::Error>(Response::new(Full::new(Bytes::from(buffer))))
                        }
                    }
                }
            });
            if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                hyper_util::rt::TokioExecutor::new(),
            )
            .serve_connection(io, svc)
            .await
            {
                tracing::warn!(error = %e, "metrics connection error");
            }
        });
    }
}

#[tokio::main]
async fn main() {
    // Tracing: JSON format with thread IDs, file/line numbers, env-filterable
    experimentation_core::telemetry::init_tracing("experimentation-pipeline");

    let port: u16 = env_or("PORT", DEFAULT_PORT).parse().expect("PORT must be u16");
    let metrics_port: u16 = env_or("METRICS_PORT", "9090")
        .parse()
        .expect("METRICS_PORT must be u16");
    let kafka_brokers = env_or("KAFKA_BROKERS", "localhost:9092");
    let kafka_linger_ms: u32 = env_or("KAFKA_LINGER_MS", "0")
        .parse()
        .expect("KAFKA_LINGER_MS must be u32");
    let bloom_daily: usize = env_or("BLOOM_EXPECTED_DAILY", "100000000")
        .parse()
        .expect("BLOOM_EXPECTED_DAILY must be usize");
    let bloom_fp: f64 = env_or("BLOOM_FP_RATE", "0.001")
        .parse()
        .expect("BLOOM_FP_RATE must be f64");
    let bloom_rotation_secs: u64 = env_or("BLOOM_ROTATION_SECS", "3600")
        .parse()
        .expect("BLOOM_ROTATION_SECS must be u64");
    let buffer_dir = env_or("BUFFER_DIR", "/tmp/experimentation-pipeline-buffer");
    let buffer_max_mb: u64 = env_or("BUFFER_MAX_MB", "100")
        .parse()
        .expect("BUFFER_MAX_MB must be u64");

    info!(
        port,
        metrics_port,
        kafka_brokers = %kafka_brokers,
        bloom_daily,
        bloom_fp,
        bloom_rotation_secs,
        buffer_dir = %buffer_dir,
        buffer_max_mb,
        "Starting M2 Event Pipeline"
    );

    // Prometheus registry
    let registry = Registry::new();

    let kafka_config = KafkaConfig {
        brokers: kafka_brokers,
        linger_ms: kafka_linger_ms,
        ..Default::default()
    };

    let producer = EventProducer::new(&kafka_config).expect("Failed to create Kafka producer");

    let dedup_config = DedupConfig {
        items_per_interval: bloom_daily / 24,
        fp_rate: bloom_fp,
        rotation_interval_secs: bloom_rotation_secs,
    };
    let dedup_metrics = DedupMetrics::new(&registry);
    let dedup = EventDedup::with_config(dedup_config, dedup_metrics);

    let pipeline_metrics = PipelineMetrics::new(&registry);

    let buffer_config = BufferConfig {
        dir: PathBuf::from(buffer_dir),
        max_size_bytes: buffer_max_mb * 1024 * 1024,
    };
    let disk_buffer = DiskBuffer::new(buffer_config).expect("Failed to initialize disk buffer");

    let service = IngestionServiceImpl::new(producer, dedup, pipeline_metrics, disk_buffer);

    // Crash-only: replay any buffered events from previous run before accepting traffic
    service.replay_buffer().await;

    // Spawn Prometheus metrics server
    let metrics_addr: SocketAddr = ([0, 0, 0, 0], metrics_port).into();
    tokio::spawn(serve_metrics(metrics_addr, registry));

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    info!(%addr, "gRPC server listening");

    tonic::transport::Server::builder()
        .accept_http1(true)
        .layer(tonic_web::GrpcWebLayer::new())
        .add_service(EventIngestionServiceServer::new(service))
        .serve(addr)
        .await
        .expect("gRPC server failed");
}
