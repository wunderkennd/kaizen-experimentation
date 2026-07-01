use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use experimentation_assignment::bandit_client::GrpcBanditClient;
use experimentation_assignment::config::Config;
use experimentation_assignment::config_cache::ConfigCache;
#[cfg(feature = "connectrpc")]
use experimentation_assignment::connect_server::ConnectAssignment;
use experimentation_assignment::http_json;
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
    let http_addr = std::env::var("HTTP_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
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

    // Connect to M4b BanditPolicyService for live arm selection.
    let bandit_client = if let Ok(m4b_addr) = std::env::var("M4B_ADDR") {
        match GrpcBanditClient::connect(&m4b_addr).await {
            Ok(client) => {
                tracing::info!(m4b_addr = %m4b_addr, "M4b bandit client connected");
                Some(client)
            }
            Err(e) => {
                tracing::warn!(
                    m4b_addr = %m4b_addr,
                    error = %e,
                    "M4b connect failed, bandit experiments use uniform random fallback",
                );
                None
            }
        }
    } else {
        tracing::warn!("M4B_ADDR not set, bandit experiments use uniform random");
        None
    };

    let svc = Arc::new(AssignmentServiceImpl::new(handle, bandit_client));

    // Spawn JSON HTTP server for SDK access.
    let http_svc = svc.clone();
    tokio::spawn(async move {
        if let Err(e) = http_json::serve(http_addr, http_svc).await {
            tracing::error!(error = %e, "JSON HTTP server failed");
        }
    });

    // ADR-031 pilot: optional ConnectRPC listener (Connect + gRPC + gRPC-Web on
    // one port). Runs alongside the tonic gRPC + http_json listeners during the
    // pilot; tonic stays the default build.
    //
    // NOTE: this listener is fire-and-forget and does NOT participate in graceful
    // shutdown — on ctrl+c the runtime drops it without draining in-flight requests.
    // Acceptable for the pilot; wiring it to `shutdown` (CancellationToken) is
    // deferred to production hardening (post-pilot).
    #[cfg(feature = "connectrpc")]
    {
        use experimentation_proto_connect::experimentation::assignment::v1::AssignmentServiceExt;

        let connect_addr: std::net::SocketAddr = std::env::var("CONNECTRPC_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:50061".to_string())
            .parse()?;
        let connect_svc = Arc::new(ConnectAssignment::new(svc.clone()));
        let router = connect_svc.register(connectrpc::Router::new());
        tokio::spawn(async move {
            tracing::info!(%connect_addr, "starting ConnectRPC pilot listener (ADR-031)");
            if let Err(e) = connectrpc::Server::new(router).serve(connect_addr).await {
                tracing::error!(error = %e, "ConnectRPC pilot server failed");
            }
        });
    }

    tracing::info!(%grpc_addr, "starting gRPC server");
    tonic::transport::Server::builder()
        .tcp_nodelay(true)
        .concurrency_limit_per_connection(256)
        .initial_connection_window_size(1024 * 1024)
        .initial_stream_window_size(1024 * 1024)
        .add_service(AssignmentServiceServer::from_arc(svc))
        .serve_with_shutdown(grpc_addr, async move {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown signal received");
            shutdown.cancel();
        })
        .await?;

    Ok(())
}
