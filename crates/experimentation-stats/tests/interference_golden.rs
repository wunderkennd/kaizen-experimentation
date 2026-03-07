//! Golden-file integration tests for interference analysis.
//!
//! Validates JSD, Jaccard, Gini, and spillover detection against
//! reference values. Deterministic inputs — tolerance 1e-6.
//!
//! Run: cargo test -p experimentation-stats --test interference_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test interference_golden

use experimentation_stats::interference::*;
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenInterference {
    test_name: String,
    description: String,
    input: InterferenceInput,
    alpha: f64,
    js_threshold: f64,
    expected: GoldenInterferenceExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenInterferenceExpected {
    interference_detected: bool,
    jensen_shannon_divergence: f64,
    jaccard_similarity_top_100: f64,
    treatment_gini_coefficient: f64,
    control_gini_coefficient: f64,
    treatment_catalog_coverage: f64,
    control_catalog_coverage: f64,
    spillover_count: usize,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenInterference {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenInterference) {
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
    let result = analyze_interference(&golden.input, golden.alpha, golden.js_threshold)
        .unwrap_or_else(|e| panic!("[{}] analyze_interference failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenInterferenceExpected {
            interference_detected: result.interference_detected,
            jensen_shannon_divergence: result.jensen_shannon_divergence,
            jaccard_similarity_top_100: result.jaccard_similarity_top_100,
            treatment_gini_coefficient: result.treatment_gini_coefficient,
            control_gini_coefficient: result.control_gini_coefficient,
            treatment_catalog_coverage: result.treatment_catalog_coverage,
            control_catalog_coverage: result.control_catalog_coverage,
            spillover_count: result.spillover_titles.len(),
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    assert_eq!(
        result.interference_detected, expected.interference_detected,
        "[{name}] interference_detected"
    );
    assert_close(result.jensen_shannon_divergence, expected.jensen_shannon_divergence, "jsd", name);
    assert_close(result.jaccard_similarity_top_100, expected.jaccard_similarity_top_100, "jaccard", name);
    assert_close(result.treatment_gini_coefficient, expected.treatment_gini_coefficient, "t_gini", name);
    assert_close(result.control_gini_coefficient, expected.control_gini_coefficient, "c_gini", name);
    assert_close(result.treatment_catalog_coverage, expected.treatment_catalog_coverage, "t_coverage", name);
    assert_close(result.control_catalog_coverage, expected.control_catalog_coverage, "c_coverage", name);
    assert_eq!(
        result.spillover_titles.len(), expected.spillover_count,
        "[{name}] spillover_count"
    );
}

#[test]
fn golden_interference_clear() {
    run_golden_test("interference_clear.json");
}

#[test]
fn golden_interference_none() {
    run_golden_test("interference_none.json");
}

#[test]
fn golden_interference_mixed() {
    run_golden_test("interference_mixed.json");
}
