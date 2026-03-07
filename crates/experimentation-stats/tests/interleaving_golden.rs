//! Golden-file integration tests for interleaving analysis.
//!
//! Validates sign test p-values, Bradley-Terry strengths, and win rates
//! against reference values. Tolerance 1e-6 for deterministic outputs.
//!
//! Run: cargo test -p experimentation-stats --test interleaving_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test interleaving_golden

use std::collections::HashMap;
use std::path::PathBuf;

use experimentation_stats::interleaving::*;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenInterleaving {
    test_name: String,
    description: String,
    scores: Vec<InterleavingScore>,
    alpha: f64,
    expected: GoldenInterleavingExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenInterleavingExpected {
    algorithm_win_rates: HashMap<String, f64>,
    sign_test_p_value: f64,
    algorithm_strengths: Vec<AlgorithmStrength>,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenInterleaving {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenInterleaving) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.15e}, got {actual:.15e}, diff={diff:.15e} > tol={TOLERANCE:.0e}"
    );
}

fn run_golden_test(filename: &str) {
    let mut golden = load_golden(filename);
    let result = analyze_interleaving(&golden.scores, golden.alpha)
        .unwrap_or_else(|e| panic!("[{}] analyze_interleaving failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenInterleavingExpected {
            algorithm_win_rates: result.algorithm_win_rates.clone(),
            sign_test_p_value: result.sign_test_p_value,
            algorithm_strengths: result.algorithm_strengths.clone(),
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    // Win rates.
    for (algo, &expected_rate) in &expected.algorithm_win_rates {
        let actual_rate = result.algorithm_win_rates.get(algo)
            .unwrap_or_else(|| panic!("[{name}] missing win rate for {algo}"));
        assert_close(*actual_rate, expected_rate, &format!("win_rate[{algo}]"), name);
    }

    // Sign test p-value.
    assert_close(result.sign_test_p_value, expected.sign_test_p_value, "sign_test_p_value", name);

    // Bradley-Terry strengths.
    let mut actual_strengths: Vec<&AlgorithmStrength> = result.algorithm_strengths.iter().collect();
    actual_strengths.sort_by(|a, b| a.algorithm_id.cmp(&b.algorithm_id));
    let mut expected_strengths: Vec<&AlgorithmStrength> = expected.algorithm_strengths.iter().collect();
    expected_strengths.sort_by(|a, b| a.algorithm_id.cmp(&b.algorithm_id));

    assert_eq!(actual_strengths.len(), expected_strengths.len(), "[{name}] strength count mismatch");

    for (actual, exp) in actual_strengths.iter().zip(expected_strengths.iter()) {
        assert_eq!(actual.algorithm_id, exp.algorithm_id, "[{name}] algorithm_id mismatch");
        assert_close(actual.strength, exp.strength, &format!("strength[{}]", actual.algorithm_id), name);
    }
}

#[test]
fn golden_interleaving_strong_preference() {
    run_golden_test("interleaving_strong_preference.json");
}

#[test]
fn golden_interleaving_balanced() {
    run_golden_test("interleaving_balanced.json");
}

#[test]
fn golden_interleaving_three_algorithms() {
    run_golden_test("interleaving_three_algorithms.json");
}
