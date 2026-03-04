//! Golden-file integration tests for SRM chi-squared test.
//!
//! Expected values computed via scipy.stats.chisquare(),
//! equivalent to R's chisq.test(observed, p=expected).
//!
//! Run: cargo test -p experimentation-stats --test srm_golden
//! Update golden files: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test srm_golden

use experimentation_stats::srm::srm_check;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenSrm {
    test_name: String,
    r_command: String,
    observed: HashMap<String, u64>,
    expected_fractions: HashMap<String, f64>,
    alpha: f64,
    expected: GoldenSrmExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenSrmExpected {
    chi_squared: f64,
    p_value: f64,
    is_mismatch: bool,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenSrm {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenSrm) {
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
    let result = srm_check(&golden.observed, &golden.expected_fractions, golden.alpha)
        .unwrap_or_else(|e| panic!("[{}] srm_check failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenSrmExpected {
            chi_squared: result.chi_squared,
            p_value: result.p_value,
            is_mismatch: result.is_mismatch,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    assert_close(result.chi_squared, expected.chi_squared, "chi_squared", name);
    assert_close(result.p_value, expected.p_value, "p_value", name);
    assert_eq!(
        result.is_mismatch, expected.is_mismatch,
        "[{name}] is_mismatch: expected {}, got {}",
        expected.is_mismatch, result.is_mismatch
    );
}

#[test]
fn golden_srm_no_mismatch() {
    run_golden_test("srm_no_mismatch.json");
}

#[test]
fn golden_srm_clear_mismatch() {
    run_golden_test("srm_clear_mismatch.json");
}

#[test]
fn golden_srm_three_variants() {
    run_golden_test("srm_three_variants.json");
}
