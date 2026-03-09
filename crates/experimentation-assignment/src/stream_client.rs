//! Background gRPC client that subscribes to M5's `StreamConfigUpdates`.
//!
//! Connects to the M5 management service, receives experiment config deltas,
//! and feeds them into [`ConfigCache`] for live config updates.
//! Reconnects with exponential backoff on stream errors.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

use experimentation_proto::experimentation::assignment::v1::{
    assignment_service_client::AssignmentServiceClient, StreamConfigUpdatesRequest,
};

use crate::config_cache::ConfigCache;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Background stream client that subscribes to M5 config updates.
pub struct StreamClient {
    m5_endpoint: String,
    cache: ConfigCache,
}

impl StreamClient {
    pub fn new(m5_endpoint: String, cache: ConfigCache) -> Self {
        Self { m5_endpoint, cache }
    }

    /// Run the stream loop until `shutdown` is cancelled.
    ///
    /// On disconnect, reconnects with exponential backoff (1s → 30s with jitter).
    /// On shutdown, exits cleanly.
    pub async fn run(mut self, shutdown: CancellationToken) {
        let mut backoff = INITIAL_BACKOFF;

        loop {
            if shutdown.is_cancelled() {
                tracing::info!("stream client shutting down");
                return;
            }

            match self.stream_once(&shutdown).await {
                Ok(()) => {
                    // Clean exit (shutdown requested during streaming).
                    tracing::info!("stream client exited cleanly");
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        backoff_ms = backoff.as_millis(),
                        "M5 stream error, reconnecting"
                    );
                }
            }

            // Exponential backoff with jitter.
            let jitter = Duration::from_millis(rand_jitter(backoff.as_millis() as u64));
            tokio::select! {
                _ = tokio::time::sleep(backoff + jitter) => {}
                _ = shutdown.cancelled() => {
                    tracing::info!("stream client shutting down during backoff");
                    return;
                }
            }

            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }

    /// Single stream attempt: connect, subscribe, process updates.
    /// Returns Ok(()) if shutdown was requested, Err on stream/connection error.
    async fn stream_once(
        &mut self,
        shutdown: &CancellationToken,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let channel = Channel::from_shared(self.m5_endpoint.clone())?
            .connect()
            .await?;

        let mut client = AssignmentServiceClient::new(channel);

        tracing::info!(
            endpoint = %self.m5_endpoint,
            last_version = self.cache.last_version(),
            "connected to M5, subscribing to config updates"
        );

        let request = StreamConfigUpdatesRequest {
            last_known_version: self.cache.last_version(),
        };

        let mut stream = client.stream_config_updates(request).await?.into_inner();

        // Reset backoff on successful connection.
        // (Caller manages backoff state, but we signal success by processing updates.)

        loop {
            tokio::select! {
                msg = stream.message() => {
                    match msg? {
                        Some(update) => {
                            self.cache.apply_update(&update);
                        }
                        None => {
                            tracing::info!("M5 stream ended (server closed)");
                            return Err("stream ended".into());
                        }
                    }
                }
                _ = shutdown.cancelled() => {
                    return Ok(());
                }
            }
        }
    }
}

/// Simple jitter: random value in [0, max_ms/4].
fn rand_jitter(max_ms: u64) -> u64 {
    // Use a simple deterministic approach: time-based seed.
    // Not cryptographic, just to avoid thundering herd.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos % (max_ms / 4 + 1)
}
