//! Policy service configuration from environment variables.

/// Configuration for the policy service.
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// gRPC listen address.
    pub grpc_addr: String,
    /// RocksDB data directory path.
    pub rocksdb_path: String,
    /// Depth of the policy request channel (gRPC → core).
    pub policy_channel_depth: usize,
    /// Depth of the reward update channel (Kafka → core).
    pub reward_channel_depth: usize,
    /// Number of rewards between RocksDB snapshots per experiment.
    pub snapshot_interval: u64,
    /// Maximum number of snapshots to retain per experiment.
    pub max_snapshots_per_experiment: usize,
}

impl PolicyConfig {
    /// Load configuration from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        Self {
            grpc_addr: std::env::var("POLICY_GRPC_ADDR")
                .unwrap_or_else(|_| "[::1]:50054".into()),
            rocksdb_path: std::env::var("POLICY_ROCKSDB_PATH")
                .unwrap_or_else(|_| "/tmp/experimentation-policy-rocksdb".into()),
            policy_channel_depth: parse_env("POLICY_CHANNEL_DEPTH", 10_000),
            reward_channel_depth: parse_env("REWARD_CHANNEL_DEPTH", 50_000),
            snapshot_interval: parse_env("SNAPSHOT_INTERVAL", 100),
            max_snapshots_per_experiment: parse_env("MAX_SNAPSHOTS_PER_EXPERIMENT", 10),
        }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
