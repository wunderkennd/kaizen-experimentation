//! Golden-file integration tests for bootstrap confidence intervals.
//!
//! Bootstrap values are RNG-dependent, so golden files are generated from our
//! implementation with fixed seeds. Tests verify reproducibility across runs.
//!
//! Run: cargo test -p experimentation-stats --test bootstrap_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test bootstrap_golden

use experimentation_stats::bootstrap::{bootstrap_bca, bootstrap_ci};
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenBootstrap {
    test_name: String,
    description: String,
    control: Vec<f64>,
    treatment: Vec<f64>,
    alpha: f64,
    n_resamples: usize,
    seed: u64,
    expected_percentile: GoldenBootstrapExpected,
    expected_bca: GoldenBootstrapExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenBootstrapExpected {
    effect: f64,
    ci_lower: f64,
    ci_upper: f64,
    bias: f64,
}

const TOLERANCE: f64 = 1e-4;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenBootstrap {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenBootstrap) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.10e}, got {actual:.10e}, diff={diff:.10e} > tolerance={TOLERANCE:.0e}"
    );
}

fn run_golden_test(filename: &str) {
    let mut golden = load_golden(filename);

    let pct = bootstrap_ci(
        &golden.control,
        &golden.treatment,
        golden.alpha,
        golden.n_resamples,
        golden.seed,
    )
    .unwrap_or_else(|e| panic!("[{}] bootstrap_ci failed: {e}", golden.test_name));

    let bca = bootstrap_bca(
        &golden.control,
        &golden.treatment,
        golden.alpha,
        golden.n_resamples,
        golden.seed,
    )
    .unwrap_or_else(|e| panic!("[{}] bootstrap_bca failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected_percentile = GoldenBootstrapExpected {
            effect: pct.effect,
            ci_lower: pct.ci_lower,
            ci_upper: pct.ci_upper,
            bias: pct.bias,
        };
        golden.expected_bca = GoldenBootstrapExpected {
            effect: bca.effect,
            ci_lower: bca.ci_lower,
            ci_upper: bca.ci_upper,
            bias: bca.bias,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {filename}", golden.test_name);
        return;
    }

    let name = &golden.test_name;

    let ep = &golden.expected_percentile;
    assert_close(pct.effect, ep.effect, "percentile.effect", name);
    assert_close(pct.ci_lower, ep.ci_lower, "percentile.ci_lower", name);
    assert_close(pct.ci_upper, ep.ci_upper, "percentile.ci_upper", name);
    assert_close(pct.bias, ep.bias, "percentile.bias", name);

    let eb = &golden.expected_bca;
    assert_close(bca.effect, eb.effect, "bca.effect", name);
    assert_close(bca.ci_lower, eb.ci_lower, "bca.ci_lower", name);
    assert_close(bca.ci_upper, eb.ci_upper, "bca.ci_upper", name);
    assert_close(bca.bias, eb.bias, "bca.bias", name);
}

#[test]
fn golden_bootstrap_normal_effect() {
    run_golden_test("bootstrap_normal_effect.json");
}

#[test]
fn golden_bootstrap_skewed_effect() {
    run_golden_test("bootstrap_skewed_effect.json");
}

#[test]
fn golden_bootstrap_no_effect() {
    run_golden_test("bootstrap_no_effect.json");
}

#[test]
fn bootstrap_no_effect_ci_contains_zero() {
    let control = vec![5.1, 4.8, 5.3, 4.9, 5.2, 5.0, 4.7, 5.4, 5.1, 4.6, 5.3, 4.8];
    let treatment = vec![5.0, 5.2, 4.7, 5.1, 4.9, 5.3, 4.8, 5.0, 5.2, 4.6, 5.1, 5.4];
    let result = bootstrap_ci(&control, &treatment, 0.05, 10000, 42).unwrap();
    assert!(
        result.ci_lower <= 0.0 && result.ci_upper >= 0.0,
        "Percentile CI [{}, {}] should contain 0 for null effect",
        result.ci_lower,
        result.ci_upper
    );

    let result = bootstrap_bca(&control, &treatment, 0.05, 10000, 42).unwrap();
    assert!(
        result.ci_lower <= 0.0 && result.ci_upper >= 0.0,
        "BCa CI [{}, {}] should contain 0 for null effect",
        result.ci_lower,
        result.ci_upper
    );
}

#[test]
fn bootstrap_ci_narrows_with_larger_samples() {
    let small_c: Vec<f64> = (1..=5).map(|x| x as f64).collect();
    let small_t: Vec<f64> = (2..=6).map(|x| x as f64).collect();
    let large_c: Vec<f64> = (1..=50).map(|x| x as f64 / 10.0).collect();
    let large_t: Vec<f64> = (11..=60).map(|x| x as f64 / 10.0).collect();

    let small = bootstrap_ci(&small_c, &small_t, 0.05, 5000, 42).unwrap();
    let large = bootstrap_ci(&large_c, &large_t, 0.05, 5000, 42).unwrap();

    let small_width = small.ci_upper - small.ci_lower;
    let large_width = large.ci_upper - large.ci_lower;
    assert!(
        large_width < small_width,
        "CI should narrow with more data: small_width={small_width}, large_width={large_width}"
    );
}
