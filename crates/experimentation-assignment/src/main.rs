#![allow(clippy::result_large_err)]
#![allow(clippy::double_must_use)]

use std::path::Path;
use std::sync::Arc;

use axum::serve::ListenerExt;
use tokio_util::sync::CancellationToken;

use experimentation_assignment::bandit_client::GrpcBanditClient;
use experimentation_assignment::config::Config;
use experimentation_assignment::config_cache::ConfigCache;
use experimentation_assignment::connect_server;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_assignment::stream_client::StreamClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    experimentation_core::telemetry::init_tracing("experimentation-assignment");

    let config_path =
        std::env::var("CONFIG_PATH").unwrap_or_else(|_| "dev/config.json".to_string());
    // One listener serves Connect (JSON + binary), gRPC and gRPC-Web (ADR-032).
    // The variable keeps its pre-flip name so existing deployments need no
    // change; HTTP_ADDR and CONNECTRPC_ADDR are gone with their listeners.
    let addr: std::net::SocketAddr = std::env::var("GRPC_ADDR")
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

    // grpc.health.v1 rides the same listener, so `grpc_health_probe -addr=:50051`
    // (ECS HEALTHCHECK) and load-balancer gRPC checks keep working; both the
    // whole server ("") and AssignmentService start SERVING, as with
    // tonic-health. M5 stream and M4b client are optional/best-effort (both
    // spawned above with warnings on failure), so "config loaded, service
    // assembled" IS the honest ready state.
    let (app, health) = connect_server::app(svc);

    let signal = async move {
        shutdown_signal().await;
        tracing::info!("shutdown signal received");
        // Report NOT_SERVING first so probes stop routing here while
        // in-flight requests drain.
        health.shutdown();
        shutdown.cancel();
    };

    // axum::serve speaks HTTP/1.1 and h2c (gRPC needs HTTP/2 prior knowledge)
    // on one port; hyper's HTTP/2 defaults match the 1 MiB windows the tonic
    // server set by hand.
    let listener = tokio::net::TcpListener::bind(addr).await?.tap_io(|tcp| {
        if let Err(e) = tcp.set_nodelay(true) {
            tracing::warn!(error = %e, "failed to set TCP_NODELAY");
        }
    });

    tracing::info!(%addr, "starting Connect + gRPC + gRPC-Web server");
    axum::serve(listener, app)
        .with_graceful_shutdown(signal)
        .await?;

    Ok(())
}

/// Resolves on ctrl+c, or SIGTERM (what ECS and Cloud Run send on stop).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "SIGTERM handler unavailable; ctrl+c only");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
