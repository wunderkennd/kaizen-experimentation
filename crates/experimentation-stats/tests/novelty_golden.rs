//! Golden-file integration tests for novelty/primacy analysis.
//!
//! Validates Gauss-Newton exponential decay fitting against known
//! parameters. Tolerance 1e-4 (fitted params, not exact).
//!
//! Run: cargo test -p experimentation-stats --test novelty_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test novelty_golden

use experimentation_stats::novelty::*;
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenNovelty {
    test_name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    true_params: Option<TrueParams>,
    daily_effects: Vec<DailyEffect>,
    alpha: f64,
    expected: GoldenNoveltyExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct TrueParams {
    s: f64,
    a: f64,
    d: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenNoveltyExpected {
    novelty_detected: bool,
    projected_steady_state_effect: f64,
    novelty_amplitude: f64,
    decay_constant_days: f64,
    r_squared: f64,
    is_stabilized: bool,
    amplitude_ci_lower: f64,
    amplitude_ci_upper: f64,
}

/// Tolerance for fitted parameters (nonlinear solver has some variability).
const TOLERANCE: f64 = 1e-4;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenNovelty {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenNovelty) {
    let path = golden_dir().join(filename);
    let data = serde_json::to_string_pretty(golden).expect("serialization should not fail");
    std::fs::write(&path, data)
        .unwrap_or_else(|e| panic!("Failed to write golden file {}: {e}", path.display()));
}

fn assert_close(actual: f64, expected: f64, field: &str, test_name: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff < TOLERANCE,
        "[{test_name}] {field}: expected {expected:.10e}, got {actual:.10e}, diff={diff:.10e} > tol={TOLERANCE:.0e}"
    );
}

fn run_golden_test(filename: &str) {
    let mut golden = load_golden(filename);
    let result = analyze_novelty(&golden.daily_effects, golden.alpha)
        .unwrap_or_else(|e| panic!("[{}] analyze_novelty failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenNoveltyExpected {
            novelty_detected: result.novelty_detected,
            projected_steady_state_effect: result.projected_steady_state_effect,
            novelty_amplitude: result.novelty_amplitude,
            decay_constant_days: result.decay_constant_days,
            r_squared: result.r_squared,
            is_stabilized: result.is_stabilized,
            amplitude_ci_lower: result.amplitude_ci_lower,
            amplitude_ci_upper: result.amplitude_ci_upper,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    assert_eq!(
        result.novelty_detected, expected.novelty_detected,
        "[{name}] novelty_detected"
    );
    assert_close(
        result.projected_steady_state_effect,
        expected.projected_steady_state_effect,
        "steady_state",
        name,
    );
    assert_close(
        result.novelty_amplitude,
        expected.novelty_amplitude,
        "amplitude",
        name,
    );
    assert_close(
        result.decay_constant_days,
        expected.decay_constant_days,
        "decay_constant",
        name,
    );
    assert_close(result.r_squared, expected.r_squared, "r_squared", name);
    assert_eq!(
        result.is_stabilized, expected.is_stabilized,
        "[{name}] is_stabilized"
    );

    // Verify fitted params are close to true params (within 20%) if available.
    // Nonlinear fitting with noise can deviate, so this is a sanity check only.
    if let Some(ref true_p) = golden.true_params {
        let s_err = ((result.projected_steady_state_effect - true_p.s) / true_p.s).abs();
        assert!(
            s_err < 0.20,
            "[{name}] steady_state {:.4} vs true {:.4}: error {:.1}% > 20%",
            result.projected_steady_state_effect, true_p.s, s_err * 100.0
        );
        let a_err = ((result.novelty_amplitude - true_p.a) / true_p.a).abs();
        assert!(
            a_err < 0.20,
            "[{name}] amplitude {:.4} vs true {:.4}: error {:.1}% > 20%",
            result.novelty_amplitude, true_p.a, a_err * 100.0
        );
        let d_err = ((result.decay_constant_days - true_p.d) / true_p.d).abs();
        assert!(
            d_err < 0.20,
            "[{name}] decay_constant {:.4} vs true {:.4}: error {:.1}% > 20%",
            result.decay_constant_days, true_p.d, d_err * 100.0
        );
    }
}

#[test]
fn golden_novelty_clear_decay() {
    run_golden_test("novelty_clear_decay.json");
}

#[test]
fn golden_novelty_no_effect() {
    run_golden_test("novelty_no_effect.json");
}

#[test]
fn golden_novelty_primacy() {
    run_golden_test("novelty_primacy.json");
}
