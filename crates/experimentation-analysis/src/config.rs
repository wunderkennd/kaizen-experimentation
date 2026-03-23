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
    /// PostgreSQL connection URL. None = caching disabled.
    pub database_url: Option<String>,
    /// Default mixing variance τ² for AVLM/mSPRT martingale.
    /// Controls sensitivity: larger τ² → faster detection of large effects, less of small.
    pub default_tau_sq: f64,
}

impl AnalysisConfig {
    /// Load configuration from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        Self {
            grpc_addr: std::env::var("ANALYSIS_GRPC_ADDR").unwrap_or_else(|_| "[::1]:50055".into()),
            delta_lake_path: std::env::var("DELTA_LAKE_PATH")
                .unwrap_or_else(|_| "/tmp/delta".into()),
            default_alpha: parse_env("ANALYSIS_DEFAULT_ALPHA", 0.05),
            default_js_threshold: parse_env("ANALYSIS_JS_THRESHOLD", 0.05),
            database_url: std::env::var("DATABASE_URL").ok(),
            default_tau_sq: parse_env("ANALYSIS_DEFAULT_TAU_SQ", 0.5),
        }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
