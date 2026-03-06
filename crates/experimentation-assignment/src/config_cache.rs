//! Live config cache backed by a `tokio::sync::watch` channel.
//!
//! Single producer (background M5 stream task) publishes new `Arc<Config>` snapshots.
//! Multiple consumers (gRPC handlers) read via `ConfigCacheHandle::snapshot()` — ~5ns.

use std::collections::HashMap;
use std::sync::Arc;

use experimentation_core::error::assert_finite;
use tokio::sync::watch;

use crate::config::{
    AllocationConfig, Config, ExperimentConfig, LayerConfig, VariantConfig,
};

use experimentation_proto::experimentation::common::v1::{
    Experiment, ExperimentState, ExperimentType, Variant,
};

use experimentation_proto::experimentation::assignment::v1::ConfigUpdate;

/// Producer side — owned by the background stream task.
pub struct ConfigCache {
    tx: watch::Sender<Arc<Config>>,
    experiments: HashMap<String, ExperimentConfig>,
    layers: HashMap<String, LayerConfig>,
    last_version: i64,
}

/// Reader side — cheaply cloned into each gRPC handler.
#[derive(Clone)]
pub struct ConfigCacheHandle {
    rx: watch::Receiver<Arc<Config>>,
}

impl ConfigCache {
    /// Create a new cache seeded with `initial` config.
    /// Returns the producer and a cloneable reader handle.
    pub fn new(initial: Config) -> (Self, ConfigCacheHandle) {
        let experiments: HashMap<String, ExperimentConfig> = initial
            .experiments
            .iter()
            .map(|e| (e.experiment_id.clone(), e.clone()))
            .collect();
        let layers: HashMap<String, LayerConfig> = initial
            .layers
            .iter()
            .map(|l| (l.layer_id.clone(), l.clone()))
            .collect();

        let (tx, rx) = watch::channel(Arc::new(initial));

        let cache = ConfigCache {
            tx,
            experiments,
            layers,
            last_version: 0,
        };
        let handle = ConfigCacheHandle { rx };
        (cache, handle)
    }

    /// Apply a config update from the M5 stream.
    ///
    /// Upserts or deletes the experiment, rebuilds the `Config`, and publishes
    /// the new snapshot through the watch channel.
    pub fn apply_update(&mut self, update: &ConfigUpdate) {
        let exp_id = update
            .experiment
            .as_ref()
            .map(|e| e.experiment_id.clone())
            .unwrap_or_default();

        if update.is_deletion {
            self.experiments.remove(&exp_id);
            tracing::info!(experiment_id = %exp_id, version = update.version, "experiment deleted from cache");
        } else if let Some(ref proto_exp) = update.experiment {
            let exp_config = experiment_from_proto(proto_exp, self.experiments.get(&exp_id));
            tracing::info!(
                experiment_id = %exp_id,
                state = %exp_config.state,
                version = update.version,
                "experiment upserted in cache"
            );
            self.experiments.insert(exp_id, exp_config);
        }

        if update.version > self.last_version {
            self.last_version = update.version;
        }

        self.publish();
    }

    /// Current version (for reconnect offset).
    pub fn last_version(&self) -> i64 {
        self.last_version
    }

    /// Rebuild `Config` from current maps and send to all readers.
    fn publish(&self) {
        let experiments: Vec<ExperimentConfig> = self.experiments.values().cloned().collect();
        let layers: Vec<LayerConfig> = self.layers.values().cloned().collect();
        let config = Config::from_experiments_and_layers(experiments, layers);
        // Ignore send error — means all receivers dropped.
        let _ = self.tx.send(Arc::new(config));
    }
}

impl ConfigCacheHandle {
    /// Get the current config snapshot. Atomic load + Arc clone ≈ 5ns.
    pub fn snapshot(&self) -> Arc<Config> {
        self.rx.borrow().clone()
    }

    /// Wrap a static `Arc<Config>` in a handle (for tests and backward compat).
    pub fn from_static(config: Arc<Config>) -> Self {
        let (_tx, rx) = watch::channel(config);
        ConfigCacheHandle { rx }
    }

    /// Wait for the config to change. Useful for tests.
    pub async fn changed(&mut self) -> Result<(), watch::error::RecvError> {
        self.rx.changed().await
    }
}

/// Convert a proto `Experiment` to our internal `ExperimentConfig`.
///
/// If an existing config exists for this experiment, its allocation and targeting
/// rule are preserved (the proto stream doesn't carry allocation ranges).
pub fn experiment_from_proto(
    proto: &Experiment,
    existing: Option<&ExperimentConfig>,
) -> ExperimentConfig {
    let state_str = ExperimentState::try_from(proto.state)
        .map(|s| {
            s.as_str_name()
                .strip_prefix("EXPERIMENT_STATE_")
                .unwrap_or(s.as_str_name())
                .to_string()
        })
        .unwrap_or_else(|_| "UNSPECIFIED".to_string());

    let type_str = ExperimentType::try_from(proto.r#type)
        .map(|t| {
            t.as_str_name()
                .strip_prefix("EXPERIMENT_TYPE_")
                .unwrap_or(t.as_str_name())
                .to_string()
        })
        .unwrap_or_else(|_| "UNSPECIFIED".to_string());

    let variants: Vec<VariantConfig> = proto
        .variants
        .iter()
        .map(variant_from_proto)
        .collect();

    // Preserve allocation from existing config, or default to full range.
    let allocation = existing
        .map(|e| e.allocation)
        .unwrap_or(AllocationConfig {
            start_bucket: 0,
            end_bucket: 9999,
        });

    // Preserve targeting rule from existing config (stream has only targeting_rule_id).
    let targeting_rule = existing.and_then(|e| e.targeting_rule.clone());

    ExperimentConfig {
        experiment_id: proto.experiment_id.clone(),
        name: proto.name.clone(),
        state: state_str,
        r#type: type_str,
        hash_salt: proto.hash_salt.clone(),
        layer_id: proto.layer_id.clone(),
        variants,
        allocation,
        targeting_rule,
        session_config: existing.and_then(|e| e.session_config.clone()),
    }
}

fn variant_from_proto(v: &Variant) -> VariantConfig {
    assert_finite(
        v.traffic_fraction,
        &format!("variant {}.traffic_fraction", v.variant_id),
    );
    VariantConfig {
        variant_id: v.variant_id.clone(),
        traffic_fraction: v.traffic_fraction,
        is_control: v.is_control,
        payload_json: v.payload_json.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_proto_experiment(id: &str, state: ExperimentState) -> Experiment {
        Experiment {
            experiment_id: id.to_string(),
            name: format!("Test {id}"),
            hash_salt: format!("salt_{id}"),
            layer_id: "layer_default".to_string(),
            state: state as i32,
            r#type: ExperimentType::Ab as i32,
            variants: vec![
                Variant {
                    variant_id: "control".to_string(),
                    name: "Control".to_string(),
                    traffic_fraction: 0.5,
                    is_control: true,
                    payload_json: "{}".to_string(),
                },
                Variant {
                    variant_id: "treatment".to_string(),
                    name: "Treatment".to_string(),
                    traffic_fraction: 0.5,
                    is_control: false,
                    payload_json: r#"{"color":"blue"}"#.to_string(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_experiment_from_proto_basic() {
        let proto = make_proto_experiment("exp_1", ExperimentState::Running);
        let config = experiment_from_proto(&proto, None);

        assert_eq!(config.experiment_id, "exp_1");
        assert_eq!(config.state, "RUNNING");
        assert_eq!(config.r#type, "AB");
        assert_eq!(config.hash_salt, "salt_exp_1");
        assert_eq!(config.layer_id, "layer_default");
        assert_eq!(config.variants.len(), 2);
        assert_eq!(config.variants[0].variant_id, "control");
        assert!((config.variants[0].traffic_fraction - 0.5).abs() < f64::EPSILON);
        // Default allocation when no existing config.
        assert_eq!(config.allocation.start_bucket, 0);
        assert_eq!(config.allocation.end_bucket, 9999);
        assert!(config.targeting_rule.is_none());
    }

    #[test]
    fn test_experiment_from_proto_preserves_allocation() {
        let proto = make_proto_experiment("exp_1", ExperimentState::Running);
        let existing = ExperimentConfig {
            experiment_id: "exp_1".to_string(),
            name: "Old".to_string(),
            state: "DRAFT".to_string(),
            r#type: "AB".to_string(),
            hash_salt: "old_salt".to_string(),
            layer_id: "layer_default".to_string(),
            variants: vec![],
            allocation: AllocationConfig {
                start_bucket: 100,
                end_bucket: 500,
            },
            targeting_rule: None,
        };

        let config = experiment_from_proto(&proto, Some(&existing));
        // Allocation preserved from existing.
        assert_eq!(config.allocation.start_bucket, 100);
        assert_eq!(config.allocation.end_bucket, 500);
        // But other fields updated from proto.
        assert_eq!(config.state, "RUNNING");
        assert_eq!(config.hash_salt, "salt_exp_1");
    }
}
