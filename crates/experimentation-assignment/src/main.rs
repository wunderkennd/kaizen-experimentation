use std::path::Path;
use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;
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

    let svc = AssignmentServiceImpl::new(Arc::new(config));

    tracing::info!(%grpc_addr, "starting gRPC server");
    tonic::transport::Server::builder()
        .add_service(AssignmentServiceServer::new(svc))
        .serve(grpc_addr)
        .await?;

    Ok(())
}
