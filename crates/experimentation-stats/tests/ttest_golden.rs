//! Golden-file integration tests for Welch's t-test.
//!
//! Expected values computed via scipy.stats.ttest_ind(equal_var=False),
//! equivalent to R's t.test(x, y, var.equal=FALSE).
//!
//! Run: cargo test -p experimentation-stats --test ttest_golden
//! Update golden files: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test ttest_golden

use experimentation_stats::ttest::welch_ttest;
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenTTest {
    test_name: String,
    r_command: String,
    control: Vec<f64>,
    treatment: Vec<f64>,
    alpha: f64,
    expected: GoldenExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenExpected {
    effect: f64,
    ci_lower: f64,
    ci_upper: f64,
    p_value: f64,
    is_significant: bool,
    df: f64,
    control_mean: f64,
    treatment_mean: f64,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenTTest {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenTTest) {
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
    let result = welch_ttest(&golden.control, &golden.treatment, golden.alpha)
        .unwrap_or_else(|e| panic!("[{}] welch_ttest failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenExpected {
            effect: result.effect,
            ci_lower: result.ci_lower,
            ci_upper: result.ci_upper,
            p_value: result.p_value,
            is_significant: result.is_significant,
            df: result.df,
            control_mean: result.control_mean,
            treatment_mean: result.treatment_mean,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    assert_close(result.effect, expected.effect, "effect", name);
    assert_close(result.ci_lower, expected.ci_lower, "ci_lower", name);
    assert_close(result.ci_upper, expected.ci_upper, "ci_upper", name);
    assert_close(result.p_value, expected.p_value, "p_value", name);
    assert_close(result.df, expected.df, "df", name);
    assert_close(result.control_mean, expected.control_mean, "control_mean", name);
    assert_close(result.treatment_mean, expected.treatment_mean, "treatment_mean", name);
    assert_eq!(
        result.is_significant, expected.is_significant,
        "[{name}] is_significant: expected {}, got {}",
        expected.is_significant, result.is_significant
    );
}

#[test]
fn golden_ttest_equal_variance_equal_n() {
    run_golden_test("ttest_equal_variance_equal_n.json");
}

#[test]
fn golden_ttest_unequal_n() {
    run_golden_test("ttest_unequal_n.json");
}

#[test]
fn golden_ttest_large_effect() {
    run_golden_test("ttest_large_effect.json");
}

#[test]
fn golden_ttest_small_effect() {
    run_golden_test("ttest_small_effect.json");
}

#[test]
fn golden_ttest_extreme_variance_ratio() {
    run_golden_test("ttest_extreme_variance_ratio.json");
}
