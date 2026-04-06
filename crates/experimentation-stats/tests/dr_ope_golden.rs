//! Golden-file integration tests for doubly-robust off-policy evaluation (DR-OPE).
//!
//! Validates dr_ope against deterministic MDP scenarios where the expected
//! policy value can be computed analytically.
//!
//! Run:    cargo test -p experimentation-stats --test dr_ope_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test dr_ope_golden

use experimentation_stats::orl::{dr_ope, DrOpeConfig, DrOpeResult, Trajectory, TrajectoryStep};
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

// ---------------------------------------------------------------------------
// Shared structs for golden file deserialization
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct DrOpeGolden {
    test_name: String,
    #[allow(dead_code)]
    description: String,
    config: DrOpeConfig,
    trajectories: Vec<Trajectory>,
    expected: DrOpeExpected,
    tolerance: f64,
}

#[derive(serde::Deserialize)]
struct DrOpeExpected {
    #[serde(default)]
    effect_min: Option<f64>,
    #[serde(default)]
    effect_max: Option<f64>,
    #[serde(default)]
    se_positive: Option<bool>,
    #[serde(default)]
    ci_contains_effect: Option<bool>,
    #[serde(default)]
    p_value_max: Option<f64>,
    #[serde(default)]
    effective_n_min: Option<f64>,
    #[serde(default)]
    n_trajectories: Option<usize>,
    #[serde(default)]
    dm_positive: Option<bool>,
}

fn load_golden(filename: &str) -> DrOpeGolden {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()))
}

fn run_golden_test(filename: &str) {
    let golden = load_golden(filename);
    let name = &golden.test_name;

    let result = dr_ope(&golden.trajectories, &golden.config)
        .unwrap_or_else(|e| panic!("[{name}] dr_ope failed: {e}"));

    let exp = &golden.expected;

    // --- Effect bounds ---
    if let Some(min) = exp.effect_min {
        assert!(
            result.effect >= min,
            "[{name}] effect should be >= {min}: got {}",
            result.effect
        );
    }
    if let Some(max) = exp.effect_max {
        assert!(
            result.effect <= max,
            "[{name}] effect should be <= {max}: got {}",
            result.effect
        );
    }

    // --- SE positive ---
    if exp.se_positive == Some(true) {
        assert!(
            result.se >= 0.0,
            "[{name}] se should be non-negative: got {}",
            result.se
        );
    }

    // --- CI contains effect ---
    if exp.ci_contains_effect == Some(true) {
        assert!(
            result.ci_lower <= result.effect && result.effect <= result.ci_upper,
            "[{name}] CI [{}, {}] should contain effect {}",
            result.ci_lower,
            result.ci_upper,
            result.effect
        );
    }

    // --- p-value bound ---
    if let Some(pmax) = exp.p_value_max {
        assert!(
            result.p_value <= pmax,
            "[{name}] p_value should be <= {pmax}: got {}",
            result.p_value
        );
    }

    // --- Effective N ---
    if let Some(ess_min) = exp.effective_n_min {
        assert!(
            result.effective_n >= ess_min,
            "[{name}] effective_n should be >= {ess_min}: got {}",
            result.effective_n
        );
    }

    // --- N trajectories ---
    if let Some(n) = exp.n_trajectories {
        assert_eq!(
            result.n_trajectories, n,
            "[{name}] n_trajectories mismatch"
        );
    }

    // --- DM positive ---
    if exp.dm_positive == Some(true) {
        assert!(
            result.dm_estimate > 0.0,
            "[{name}] dm_estimate should be positive: got {}",
            result.dm_estimate
        );
    }

    // --- All outputs finite ---
    assert!(result.effect.is_finite(), "[{name}] effect not finite");
    assert!(result.se.is_finite(), "[{name}] se not finite");
    assert!(result.ci_lower.is_finite(), "[{name}] ci_lower not finite");
    assert!(result.ci_upper.is_finite(), "[{name}] ci_upper not finite");
    assert!(result.t_stat.is_finite(), "[{name}] t_stat not finite");
    assert!(
        result.p_value >= 0.0 && result.p_value <= 1.0,
        "[{name}] p_value out of [0,1]: {}",
        result.p_value
    );
    assert!(result.effective_n > 0.0, "[{name}] effective_n <= 0");
    assert!(result.dm_estimate.is_finite(), "[{name}] dm_estimate not finite");
    assert!(result.ipw_estimate.is_finite(), "[{name}] ipw_estimate not finite");
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

/// Deterministic MDP: half control (r=0.2), half treatment (r=0.8).
/// Evaluate "always treat" policy. DR should yield effect ∈ [1.5, 3.5].
#[test]
fn golden_dr_ope_deterministic() {
    run_golden_test("dr_ope_deterministic.json");
}
