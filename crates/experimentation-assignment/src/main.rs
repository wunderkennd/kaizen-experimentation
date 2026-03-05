use std::path::Path;

use tokio_util::sync::CancellationToken;

use experimentation_assignment::config::Config;
use experimentation_assignment::config_cache::ConfigCache;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_assignment::stream_client::StreamClient;
use experimentation_proto::experimentation::assignment::v1::assignment_service_server::AssignmentServiceServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    experimentation_core::telemetry::init_tracing("experimentation-assignment");

    let config_path =
        std::env::var("CONFIG_PATH").unwrap_or_else(|_| "dev/config.json".to_string());
    let grpc_addr = std::env::var("GRPC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
        .parse()?;

    let config = Config::from_file(Path::new(&config_path))?;
    tracing::info!(
        experiments = config.experiments.len(),
        layers = config.layers.len(),
        "config loaded from {}",
        config_path,
    );

    let (cache, handle) = ConfigCache::new(config);
    let shutdown = CancellationToken::new();

    if let Ok(m5_addr) = std::env::var("M5_ADDR") {
        let client = StreamClient::new(m5_addr.clone(), cache);
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            client.run(shutdown_clone).await;
        });
        tracing::info!(m5_addr = %m5_addr, "M5 config stream task spawned");
    } else {
        tracing::warn!("M5_ADDR not set, running with static local config");
    }

    let svc = AssignmentServiceImpl::new(handle);

    tracing::info!(%grpc_addr, "starting gRPC server");
    tonic::transport::Server::builder()
        .add_service(AssignmentServiceServer::new(svc))
        .serve_with_shutdown(grpc_addr, async move {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown signal received");
            shutdown.cancel();
        })
        .await?;

    Ok(())
}
