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
                assert_finite(v.traffic_fraction, &format!(
                    "variant {}.traffic_fraction in experiment {}",
                    v.variant_id, exp.experiment_id,
                ));
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
