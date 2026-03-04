//! Tracing and metrics initialization.

use tracing_subscriber::{fmt, EnvFilter};

/// Initialize tracing with JSON output and env-filter.
/// Call once at service startup.
pub fn init_tracing(service_name: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    tracing::info!(service = service_name, "tracing initialized");
}
