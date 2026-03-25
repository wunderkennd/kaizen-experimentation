//! Experiment configuration loader.
//!
//! Reads from a local JSON file (dev/config.json) until M5 StreamConfigUpdates is available.

use std::collections::HashMap;
use std::path::Path;

use experimentation_core::error::assert_finite;
use serde::Deserialize;


/// Top-level configuration containing experiments and layers.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub experiments: Vec<ExperimentConfig>,
    pub layers: Vec<LayerConfig>,

    /// Indexed lookups built at load time.
    #[serde(skip)]
    pub experiments_by_id: HashMap<String, ExperimentConfig>,
    #[serde(skip)]
    pub layers_by_id: HashMap<String, LayerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExperimentConfig {
    pub experiment_id: String,
    #[serde(default)]
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub r#type: String,
    pub hash_salt: String,
    pub layer_id: String,
    pub variants: Vec<VariantConfig>,
    pub allocation: AllocationConfig,
    #[serde(default)]
    pub targeting_rule: Option<TargetingRule>,
    #[serde(default)]
    pub session_config: Option<SessionConfig>,
    #[serde(default)]
    pub interleaving_config: Option<InterleavingConfig>,
    #[serde(default)]
    pub bandit_config: Option<BanditConfig>,
    #[serde(default)]
    pub is_cumulative_holdout: bool,
    /// Meta-experiment config (ADR-013). Present when type == "META".
    #[serde(default)]
    pub meta_experiment_config: Option<MetaExperimentConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetingRule {
    #[serde(default)]
    pub groups: Vec<TargetingGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetingGroup {
    pub predicates: Vec<TargetingPredicate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetingPredicate {
    pub attribute_key: String,
    pub operator: String,
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionConfig {
    #[serde(default)]
    pub session_id_attribute: String,
    #[serde(default)]
    pub allow_cross_session_variation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InterleavingConfig {
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub algorithm_ids: Vec<String>,
    #[serde(default = "default_max_list_size")]
    pub max_list_size: usize,
}

fn default_max_list_size() -> usize {
    50
}

/// Bandit experiment configuration (MAB / CONTEXTUAL_BANDIT types).
#[derive(Debug, Clone, Deserialize)]
pub struct BanditConfig {
    /// Algorithm identifier (e.g., "THOMPSON_SAMPLING", "LINEAR_UCB").
    #[serde(default)]
    pub algorithm: String,
    /// Arm definitions — each arm maps to a variant or content placement.
    #[serde(default)]
    pub arms: Vec<BanditArmConfig>,
    /// Metric ID used as the reward signal.
    #[serde(default)]
    pub reward_metric_id: String,
    /// Context feature keys for contextual bandits.
    #[serde(default)]
    pub context_feature_keys: Vec<String>,
    /// Minimum fraction of traffic per arm (prevents starvation). Default 0.1.
    #[serde(default = "default_min_exploration")]
    pub min_exploration_fraction: f64,
    /// Uniform-random observations before policy activates. Default 1000.
    #[serde(default = "default_warmup_observations")]
    pub warmup_observations: i32,
    /// Content ID for cold-start bandit experiments.
    #[serde(default)]
    pub content_id: Option<String>,
    /// Cold-start exploration window in days.
    #[serde(default)]
    pub cold_start_window_days: Option<i32>,
}

fn default_min_exploration() -> f64 {
    0.1
}

fn default_warmup_observations() -> i32 {
    1000
}

#[derive(Debug, Clone, Deserialize)]
pub struct BanditArmConfig {
    pub arm_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub payload_json: String,
}

/// Meta-experiment config (ADR-013).
///
/// Each variant runs the same bandit arm set but with a different reward objective
/// parameterisation. M1 hashes the user to a variant, then calls M4b SelectArm
/// with key `{experiment_id}:{variant_id}` to maintain isolated per-variant policy
/// state in RocksDB.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MetaExperimentConfig {
    /// Base bandit algorithm shared across all variants.
    #[serde(default)]
    pub base_algorithm: String,
    /// Per-variant reward objective parameterisations.
    #[serde(default)]
    pub variant_objectives: Vec<MetaVariantObjective>,
    /// Metric IDs tracked for cross-variant business outcome analysis in M4a.
    #[serde(default)]
    pub outcome_metric_ids: Vec<String>,
}

/// Maps a variant to its reward objective weights.
#[derive(Debug, Clone, Deserialize)]
pub struct MetaVariantObjective {
    /// Variant this objective applies to.
    pub variant_id: String,
    /// Metric weights summing to 1.0. Key: metric_id, Value: weight.
    #[serde(default)]
    pub reward_weights: HashMap<String, f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VariantConfig {
    pub variant_id: String,
    pub traffic_fraction: f64,
    pub is_control: bool,
    #[serde(default)]
    pub payload_json: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct AllocationConfig {
    pub start_bucket: u32,
    pub end_bucket: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayerConfig {
    pub layer_id: String,
    pub total_buckets: u32,
}

impl Config {
    /// Build config from pre-validated experiments and layers (for cache rebuilds).
    pub fn from_experiments_and_layers(
        experiments: Vec<ExperimentConfig>,
        layers: Vec<LayerConfig>,
    ) -> Self {
        let experiments_by_id = experiments
            .iter()
            .map(|e| (e.experiment_id.clone(), e.clone()))
            .collect();
        let layers_by_id = layers
            .iter()
            .map(|l| (l.layer_id.clone(), l.clone()))
            .collect();
        Config {
            experiments,
            layers,
            experiments_by_id,
            layers_by_id,
        }
    }

    /// Load config from a JSON file path.
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        Self::from_json(&json)
    }

    /// Parse config from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config: Config = serde_json::from_str(json)?;

        // Validate traffic fractions and build indexes.
        for exp in &config.experiments {
            for v in &exp.variants {
                assert_finite(
                    v.traffic_fraction,
                    &format!(
                        "variant {}.traffic_fraction in experiment {}",
                        v.variant_id, exp.experiment_id,
                    ),
                );
            }
        }

        config.experiments_by_id = config
            .experiments
            .iter()
            .map(|e| (e.experiment_id.clone(), e.clone()))
            .collect();

        config.layers_by_id = config
            .layers
            .iter()
            .map(|l| (l.layer_id.clone(), l.clone()))
            .collect();

        Ok(config)
    }
}
