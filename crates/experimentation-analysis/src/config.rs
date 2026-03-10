//! Analysis service configuration from environment variables.

/// Configuration for the analysis service (M4a).
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// gRPC listen address.
    pub grpc_addr: String,
    /// Root path for Delta Lake tables.
    pub delta_lake_path: String,
    /// Default significance level for statistical tests.
    pub default_alpha: f64,
    /// Default Jensen-Shannon divergence threshold for interference detection.
    pub default_js_threshold: f64,
}

impl AnalysisConfig {
    /// Load configuration from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        Self {
            grpc_addr: std::env::var("ANALYSIS_GRPC_ADDR")
                .unwrap_or_else(|_| "[::1]:50055".into()),
            delta_lake_path: std::env::var("DELTA_LAKE_PATH")
                .unwrap_or_else(|_| "/tmp/delta".into()),
            default_alpha: parse_env("ANALYSIS_DEFAULT_ALPHA", 0.05),
            default_js_threshold: parse_env("ANALYSIS_JS_THRESHOLD", 0.05),
        }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
