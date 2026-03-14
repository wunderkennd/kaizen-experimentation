//! Golden-file integration tests for clustered standard errors.
//!
//! Validated against R: sandwich::vcovCL(lm(y ~ treatment), cluster = ~user_id, type = "HC1").
//!
//! Run: cargo test -p experimentation-stats --test clustering_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test clustering_golden

use experimentation_stats::clustering::{clustered_se, ClusteredObservation};
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[derive(serde::Deserialize)]
struct ClusteringGolden {
    test_name: String,
    observations: Vec<ClusteredObservation>,
    alpha: f64,
    expected: ClusteringExpected,
}

#[derive(serde::Deserialize)]
struct ClusteringExpected {
    #[serde(default)]
    clustered_se_greater_than_naive: Option<bool>,
    #[serde(default)]
    clustered_se_much_greater_than_naive: Option<bool>,
    #[serde(default)]
    design_effect_greater_than_1: Option<bool>,
    #[serde(default)]
    design_effect_greater_than_3: Option<bool>,
    #[serde(default)]
    design_effect_near_1: Option<bool>,
    #[serde(default)]
    naive_se_close_to_clustered_se: Option<bool>,
    #[serde(rename = "naive_p_value_less_than_0.01", default)]
    naive_p_value_less_than_001: Option<bool>,
    #[serde(rename = "clustered_p_value_less_than_0.05", default)]
    clustered_p_value_less_than_005: Option<bool>,
}

fn run_clustering_golden(filename: &str) {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    let golden: ClusteringGolden = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

    let result = clustered_se(&golden.observations, golden.alpha)
        .unwrap_or_else(|e| panic!("[{}] clustered_se failed: {e}", golden.test_name));

    let name = &golden.test_name;

    eprintln!(
        "[{name}] naive_se={}, clustered_se={}, design_effect={}, naive_p={}, clustered_p={}",
        result.naive_se,
        result.clustered_se,
        result.design_effect,
        result.naive_p_value,
        result.clustered_p_value
    );

    // All outputs must be finite.
    assert!(result.naive_se.is_finite(), "[{name}] naive_se not finite");
    assert!(
        result.clustered_se.is_finite(),
        "[{name}] clustered_se not finite"
    );
    assert!(
        result.design_effect.is_finite(),
        "[{name}] design_effect not finite"
    );
    assert!(
        result.naive_p_value.is_finite(),
        "[{name}] naive_p_value not finite"
    );
    assert!(
        result.clustered_p_value.is_finite(),
        "[{name}] clustered_p_value not finite"
    );

    // P-values in [0, 1].
    assert!(
        result.naive_p_value >= 0.0 && result.naive_p_value <= 1.0,
        "[{name}] naive_p_value not in [0,1]: {}",
        result.naive_p_value
    );
    assert!(
        result.clustered_p_value >= 0.0 && result.clustered_p_value <= 1.0,
        "[{name}] clustered_p_value not in [0,1]: {}",
        result.clustered_p_value
    );

    if golden.expected.clustered_se_greater_than_naive == Some(true) {
        assert!(
            result.clustered_se >= result.naive_se,
            "[{name}] clustered_se ({}) < naive_se ({})",
            result.clustered_se,
            result.naive_se
        );
    }

    if golden.expected.clustered_se_much_greater_than_naive == Some(true) {
        assert!(
            result.clustered_se > result.naive_se * 1.5,
            "[{name}] clustered_se ({}) not much > naive_se ({})",
            result.clustered_se,
            result.naive_se
        );
    }

    if golden.expected.design_effect_greater_than_1 == Some(true) {
        assert!(
            result.design_effect > 1.0,
            "[{name}] design_effect ({}) should be > 1.0",
            result.design_effect
        );
    }

    if golden.expected.design_effect_greater_than_3 == Some(true) {
        assert!(
            result.design_effect > 3.0,
            "[{name}] design_effect ({}) should be > 3.0",
            result.design_effect
        );
    }

    if golden.expected.design_effect_near_1 == Some(true) {
        assert!(
            (result.design_effect - 1.0).abs() < 0.5,
            "[{name}] design_effect ({}) should be near 1.0",
            result.design_effect
        );
    }

    if golden.expected.naive_se_close_to_clustered_se == Some(true) {
        let ratio = result.clustered_se / result.naive_se;
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "[{name}] naive_se ({}) and clustered_se ({}) should be close (ratio={})",
            result.naive_se,
            result.clustered_se,
            ratio
        );
    }

    if golden.expected.naive_p_value_less_than_001 == Some(true) {
        assert!(
            result.naive_p_value < 0.01,
            "[{name}] naive_p_value ({}) should be < 0.01",
            result.naive_p_value
        );
    }

    if golden.expected.clustered_p_value_less_than_005 == Some(true) {
        assert!(
            result.clustered_p_value < 0.05,
            "[{name}] clustered_p_value ({}) should be < 0.05",
            result.clustered_p_value
        );
    }
}

#[test]
fn golden_clustering_moderate_icc() {
    run_clustering_golden("clustering_moderate_icc.json");
}

#[test]
fn golden_clustering_high_icc() {
    run_clustering_golden("clustering_high_icc.json");
}

#[test]
fn golden_clustering_no_clustering() {
    run_clustering_golden("clustering_no_clustering.json");
}
