//! M2 Event Pipeline — gRPC ingestion server.
//!
//! Crash-only design: no SIGTERM handler, no graceful shutdown.
//! On restart, the Bloom filter resets (brief dedup gap accepted per design doc).
//! Kafka idempotent producer ensures no duplicates on the broker side.

mod kafka;
mod service;

use std::net::SocketAddr;

use experimentation_ingest::dedup::{DedupConfig, DedupMetrics, EventDedup};
use experimentation_proto::pipeline::event_ingestion_service_server::EventIngestionServiceServer;
use prometheus::Registry;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use crate::kafka::{EventProducer, KafkaConfig};
use crate::service::IngestionServiceImpl;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// Serve Prometheus metrics on a separate HTTP endpoint.
async fn serve_metrics(addr: SocketAddr, registry: Registry) {
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::service::service_fn;
    use hyper::{Request, Response};
    use hyper_util::rt::TokioIo;
    use prometheus::Encoder;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(addr).await.expect("metrics bind failed");
    info!(%addr, "Prometheus metrics endpoint listening");

    loop {
        let (stream, _) = listener.accept().await.expect("metrics accept failed");
        let io = TokioIo::new(stream);
        let registry = registry.clone();

        tokio::spawn(async move {
            let svc = service_fn(move |_req: Request<hyper::body::Incoming>| {
                let registry = registry.clone();
                async move {
                    let encoder = prometheus::TextEncoder::new();
                    let metric_families = registry.gather();
                    let mut buffer = Vec::new();
                    encoder.encode(&metric_families, &mut buffer).unwrap();
                    Ok::<_, hyper::Error>(Response::new(Full::new(Bytes::from(buffer))))
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
    // Tracing: JSON format, filterable via RUST_LOG env
    fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let port: u16 = env_or("PORT", "50051").parse().expect("PORT must be u16");
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

    info!(
        port,
        metrics_port,
        kafka_brokers = %kafka_brokers,
        bloom_daily,
        bloom_fp,
        bloom_rotation_secs,
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
    let service = IngestionServiceImpl::new(producer, dedup);

    // Spawn Prometheus metrics server
    let metrics_addr: SocketAddr = ([0, 0, 0, 0], metrics_port).into();
    tokio::spawn(serve_metrics(metrics_addr, registry));

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    info!(%addr, "gRPC server listening");

    tonic::transport::Server::builder()
        .add_service(EventIngestionServiceServer::new(service))
        .serve(addr)
        .await
        .expect("gRPC server failed");
}
