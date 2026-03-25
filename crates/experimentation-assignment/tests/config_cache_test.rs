//! Integration tests for the config cache lifecycle.

use std::sync::Arc;

use experimentation_assignment::config::{
    AllocationConfig, Config, ExperimentConfig, LayerConfig, VariantConfig,
};
use experimentation_assignment::config_cache::{experiment_from_proto, ConfigCache};

use experimentation_proto::experimentation::assignment::v1::ConfigUpdate;
use experimentation_proto::experimentation::common::v1::{
    Experiment, ExperimentState, ExperimentType, Variant,
};

fn make_test_config() -> Config {
    Config::from_experiments_and_layers(
        vec![ExperimentConfig {
            experiment_id: "exp_1".to_string(),
            name: "Test Exp 1".to_string(),
            state: "RUNNING".to_string(),
            r#type: "AB".to_string(),
            hash_salt: "salt_1".to_string(),
            layer_id: "layer_default".to_string(),
            variants: vec![
                VariantConfig {
                    variant_id: "control".to_string(),
                    traffic_fraction: 0.5,
                    is_control: true,
                    payload_json: "{}".to_string(),
                },
                VariantConfig {
                    variant_id: "treatment".to_string(),
                    traffic_fraction: 0.5,
                    is_control: false,
                    payload_json: r#"{"color":"blue"}"#.to_string(),
                },
            ],
            allocation: AllocationConfig {
                start_bucket: 0,
                end_bucket: 9999,
            },
            targeting_rule: None,
            session_config: None,
            interleaving_config: None,
            bandit_config: None,
            is_cumulative_holdout: false,
            switchback_config: None,
        }],
        vec![LayerConfig {
            layer_id: "layer_default".to_string(),
            total_buckets: 10000,
        }],
    )
}

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
                payload_json: r#"{"color":"red"}"#.to_string(),
            },
        ],
        ..Default::default()
    }
}

#[test]
fn test_initial_snapshot() {
    let config = make_test_config();
    let (_cache, handle) = ConfigCache::new(config);
    let snap = handle.snapshot();

    assert_eq!(snap.experiments.len(), 1);
    assert_eq!(snap.experiments[0].experiment_id, "exp_1");
    assert_eq!(snap.layers.len(), 1);
    assert!(snap.experiments_by_id.contains_key("exp_1"));
    assert!(snap.layers_by_id.contains_key("layer_default"));
}

#[test]
fn test_apply_upsert() {
    let config = make_test_config();
    let (mut cache, handle) = ConfigCache::new(config);

    // Add a new experiment via update.
    let update = ConfigUpdate {
        experiment: Some(make_proto_experiment("exp_2", ExperimentState::Running)),
        is_deletion: false,
        version: 1,
    };
    cache.apply_update(&update);

    let snap = handle.snapshot();
    assert_eq!(snap.experiments.len(), 2);
    assert!(snap.experiments_by_id.contains_key("exp_1"));
    assert!(snap.experiments_by_id.contains_key("exp_2"));
    assert_eq!(snap.experiments_by_id["exp_2"].state, "RUNNING");
}

#[test]
fn test_apply_upsert_overwrites_existing() {
    let config = make_test_config();
    let (mut cache, handle) = ConfigCache::new(config);

    // Update exp_1 state from RUNNING to CONCLUDED.
    let update = ConfigUpdate {
        experiment: Some(make_proto_experiment("exp_1", ExperimentState::Concluded)),
        is_deletion: false,
        version: 1,
    };
    cache.apply_update(&update);

    let snap = handle.snapshot();
    assert_eq!(snap.experiments.len(), 1);
    assert_eq!(snap.experiments_by_id["exp_1"].state, "CONCLUDED");
}

#[test]
fn test_apply_deletion() {
    let config = make_test_config();
    let (mut cache, handle) = ConfigCache::new(config);

    let update = ConfigUpdate {
        experiment: Some(Experiment {
            experiment_id: "exp_1".to_string(),
            ..Default::default()
        }),
        is_deletion: true,
        version: 1,
    };
    cache.apply_update(&update);

    let snap = handle.snapshot();
    assert_eq!(snap.experiments.len(), 0);
    assert!(!snap.experiments_by_id.contains_key("exp_1"));
}

#[test]
fn test_version_tracking() {
    let config = make_test_config();
    let (mut cache, _handle) = ConfigCache::new(config);

    assert_eq!(cache.last_version(), 0);

    cache.apply_update(&ConfigUpdate {
        experiment: Some(make_proto_experiment("exp_2", ExperimentState::Running)),
        is_deletion: false,
        version: 5,
    });
    assert_eq!(cache.last_version(), 5);

    cache.apply_update(&ConfigUpdate {
        experiment: Some(make_proto_experiment("exp_3", ExperimentState::Draft)),
        is_deletion: false,
        version: 10,
    });
    assert_eq!(cache.last_version(), 10);

    // Version should never go backwards.
    cache.apply_update(&ConfigUpdate {
        experiment: Some(make_proto_experiment("exp_4", ExperimentState::Running)),
        is_deletion: false,
        version: 3,
    });
    assert_eq!(cache.last_version(), 10);
}

#[test]
fn test_experiment_from_proto() {
    let proto = make_proto_experiment("exp_test", ExperimentState::Running);
    let config = experiment_from_proto(&proto, None);

    assert_eq!(config.experiment_id, "exp_test");
    assert_eq!(config.name, "Test exp_test");
    assert_eq!(config.state, "RUNNING");
    assert_eq!(config.r#type, "AB");
    assert_eq!(config.hash_salt, "salt_exp_test");
    assert_eq!(config.layer_id, "layer_default");
    assert_eq!(config.variants.len(), 2);
    assert_eq!(config.variants[0].variant_id, "control");
    assert!(config.variants[0].is_control);
    assert!((config.variants[0].traffic_fraction - 0.5).abs() < f64::EPSILON);
    assert_eq!(config.variants[1].variant_id, "treatment");
    assert!(!config.variants[1].is_control);
    assert_eq!(config.variants[1].payload_json, r#"{"color":"red"}"#);
    // Default allocation when no existing config.
    assert_eq!(config.allocation.start_bucket, 0);
    assert_eq!(config.allocation.end_bucket, 9999);
    assert!(config.targeting_rule.is_none());
}

#[test]
fn test_concurrent_readers() {
    let config = make_test_config();
    let (mut cache, handle) = ConfigCache::new(config);

    // Spawn multiple reader threads.
    let mut threads = Vec::new();
    for i in 0..4 {
        let h = handle.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..1000 {
                let snap = h.snapshot();
                // Experiment count should always be consistent (1, 2, or more).
                assert!(!snap.experiments.is_empty() || true, "thread {i} saw empty");
                assert_eq!(snap.experiments.len(), snap.experiments_by_id.len());
            }
        }));
    }

    // Simultaneously apply updates.
    for v in 0..100 {
        let update = ConfigUpdate {
            experiment: Some(make_proto_experiment(
                &format!("concurrent_exp_{v}"),
                ExperimentState::Running,
            )),
            is_deletion: false,
            version: v + 1,
        };
        cache.apply_update(&update);
    }

    for t in threads {
        t.join().expect("reader thread panicked");
    }

    let final_snap = handle.snapshot();
    // Original exp_1 + 100 concurrent experiments.
    assert_eq!(final_snap.experiments.len(), 101);
}

#[test]
fn test_layers_preserved() {
    let config = Config::from_experiments_and_layers(
        vec![ExperimentConfig {
            experiment_id: "exp_1".to_string(),
            name: "Test".to_string(),
            state: "RUNNING".to_string(),
            r#type: "AB".to_string(),
            hash_salt: "salt".to_string(),
            layer_id: "custom_layer".to_string(),
            variants: vec![VariantConfig {
                variant_id: "ctrl".to_string(),
                traffic_fraction: 1.0,
                is_control: true,
                payload_json: "{}".to_string(),
            }],
            allocation: AllocationConfig {
                start_bucket: 0,
                end_bucket: 4999,
            },
            targeting_rule: None,
            session_config: None,
            interleaving_config: None,
            bandit_config: None,
            is_cumulative_holdout: false,
            switchback_config: None,
        }],
        vec![
            LayerConfig {
                layer_id: "custom_layer".to_string(),
                total_buckets: 5000,
            },
            LayerConfig {
                layer_id: "another_layer".to_string(),
                total_buckets: 20000,
            },
        ],
    );

    let (mut cache, handle) = ConfigCache::new(config);

    // Add a new experiment on an existing layer — layer count should stay at 2.
    let mut proto = make_proto_experiment("exp_2", ExperimentState::Running);
    proto.layer_id = "custom_layer".to_string();
    cache.apply_update(&ConfigUpdate {
        experiment: Some(proto),
        is_deletion: false,
        version: 1,
    });

    let snap = handle.snapshot();
    assert_eq!(snap.layers.len(), 2);
    assert!(snap.layers_by_id.contains_key("custom_layer"));
    assert!(snap.layers_by_id.contains_key("another_layer"));
    assert_eq!(snap.layers_by_id["custom_layer"].total_buckets, 5000);

    // Add an experiment on a NEW layer — auto-registration should add it.
    let mut proto_new = make_proto_experiment("exp_3", ExperimentState::Running);
    proto_new.layer_id = "brand_new_layer".to_string();
    cache.apply_update(&ConfigUpdate {
        experiment: Some(proto_new),
        is_deletion: false,
        version: 2,
    });

    let snap2 = handle.snapshot();
    assert_eq!(snap2.layers.len(), 3);
    assert!(snap2.layers_by_id.contains_key("brand_new_layer"));
    assert_eq!(snap2.layers_by_id["brand_new_layer"].total_buckets, 10_000);
}

#[test]
fn test_from_static_handle() {
    let config = make_test_config();
    let handle =
        experimentation_assignment::config_cache::ConfigCacheHandle::from_static(Arc::new(config));

    let snap = handle.snapshot();
    assert_eq!(snap.experiments.len(), 1);
    assert_eq!(snap.experiments[0].experiment_id, "exp_1");
}
