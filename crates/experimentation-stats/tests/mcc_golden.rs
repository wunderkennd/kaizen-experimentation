//! Golden-file integration tests for multiple comparison correction.
//!
//! Expected values computed via R's `p.adjust()` (methods "BH" and "bonferroni").
//!
//! Run: cargo test -p experimentation-stats --test mcc_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test mcc_golden

use experimentation_stats::multiple_comparison::{
    benjamini_hochberg, bonferroni, CorrectionMethod,
};
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenMcc {
    test_name: String,
    reference_command: String,
    method: String,
    p_values: Vec<f64>,
    threshold: f64,
    expected: GoldenMccExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenMccExpected {
    p_values_adjusted: Vec<f64>,
    rejected: Vec<bool>,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenMcc {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenMcc) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.15e}, got {actual:.15e}, diff={diff:.15e} > tolerance={TOLERANCE:.0e}"
    );
}

fn run_golden_test(filename: &str) {
    let mut golden = load_golden(filename);

    let result = match golden.method.as_str() {
        "BenjaminiHochberg" => benjamini_hochberg(&golden.p_values, golden.threshold)
            .unwrap_or_else(|e| panic!("[{}] benjamini_hochberg failed: {e}", golden.test_name)),
        "Bonferroni" => bonferroni(&golden.p_values, golden.threshold)
            .unwrap_or_else(|e| panic!("[{}] bonferroni failed: {e}", golden.test_name)),
        other => panic!("Unknown method: {other}"),
    };

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenMccExpected {
            p_values_adjusted: result.p_values_adjusted.clone(),
            rejected: result.rejected.clone(),
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {filename}", golden.test_name);
        return;
    }

    let name = &golden.test_name;
    let expected = &golden.expected;

    let expected_method = match golden.method.as_str() {
        "BenjaminiHochberg" => CorrectionMethod::BenjaminiHochberg,
        "Bonferroni" => CorrectionMethod::Bonferroni,
        _ => unreachable!(),
    };
    assert_eq!(result.method, expected_method, "[{name}] method mismatch");

    assert_eq!(
        result.p_values_adjusted.len(),
        expected.p_values_adjusted.len(),
        "[{name}] adjusted p-value count mismatch"
    );
    for (i, (&actual, &exp)) in result
        .p_values_adjusted
        .iter()
        .zip(expected.p_values_adjusted.iter())
        .enumerate()
    {
        assert_close(actual, exp, &format!("p_values_adjusted[{i}]"), name);
    }

    assert_eq!(
        result.rejected, expected.rejected,
        "[{name}] rejected mismatch: expected {:?}, got {:?}",
        expected.rejected, result.rejected
    );
}

#[test]
fn golden_mcc_bh_10_pvalues() {
    run_golden_test("mcc_bh_10_pvalues.json");
}

#[test]
fn golden_mcc_bonferroni_5_pvalues() {
    run_golden_test("mcc_bonferroni_5_pvalues.json");
}

#[test]
fn golden_mcc_bh_all_significant() {
    run_golden_test("mcc_bh_all_significant.json");
}
