//! Golden-file integration tests for mSPRT and GST sequential testing.
//!
//! GST boundaries validated against Armitage-McPherson-Rowe recursive integration
//! (equivalent to R's gsDesign package) to 4 decimal places per ADR-004.
//!
//! Run: cargo test -p experimentation-stats --test sequential_golden
//! Update: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test sequential_golden

use experimentation_stats::sequential::{
    gst_boundaries, msprt_normal, spending_function_alpha, SpendingFunction,
};
use std::path::PathBuf;

// ----- mSPRT golden tests -----

#[derive(serde::Deserialize)]
struct GoldenMsprt {
    test_name: String,
    z_stat: f64,
    n: f64,
    sigma_sq: f64,
    tau_sq: f64,
    alpha: f64,
    expected: MsprtExpected,
}

#[derive(serde::Deserialize)]
struct MsprtExpected {
    lambda: f64,
    p_value: f64,
    boundary_crossed: bool,
}

// ----- GST golden tests -----

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenGst {
    test_name: String,
    spending_function: String,
    planned_looks: u32,
    overall_alpha: f64,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    r_command: Option<String>,
    boundaries: Vec<GstBoundary>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GstBoundary {
    look: u32,
    information_fraction: f64,
    cumulative_alpha: f64,
    incremental_alpha: f64,
    critical_value: f64,
}

const TOLERANCE: f64 = 1e-4; // ADR-004: GST validated to 4 decimal places

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_msprt(filename: &str) -> GoldenMsprt {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn load_gst(filename: &str) -> GoldenGst {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_gst_golden(filename: &str, golden: &GoldenGst) {
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

fn run_msprt_golden(filename: &str) {
    let golden = load_msprt(filename);
    let result = msprt_normal(
        golden.z_stat,
        golden.n,
        golden.sigma_sq,
        golden.tau_sq,
        golden.alpha,
    )
    .unwrap_or_else(|e| panic!("[{}] msprt_normal failed: {e}", golden.test_name));

    let name = &golden.test_name;
    assert_close(result.lambda, golden.expected.lambda, "lambda", name);
    assert_close(result.p_value, golden.expected.p_value, "p_value", name);
    assert_eq!(
        result.boundary_crossed, golden.expected.boundary_crossed,
        "[{name}] boundary_crossed mismatch"
    );
}

fn run_gst_golden(filename: &str) {
    let mut golden = load_gst(filename);
    let spending = match golden.spending_function.as_str() {
        "OBrienFleming" => SpendingFunction::OBrienFleming,
        "Pocock" => SpendingFunction::Pocock,
        other => panic!("Unknown spending function: {other}"),
    };

    // Compute boundaries via recursive integration
    let bounds = gst_boundaries(golden.planned_looks, golden.overall_alpha, spending)
        .unwrap_or_else(|e| panic!("[{}] gst_boundaries failed: {e}", golden.test_name));

    assert_eq!(bounds.len(), golden.boundaries.len());

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        // Update golden file with current Rust output
        let mut prev_cum = 0.0;
        for (i, crit) in bounds.iter().enumerate() {
            let t = golden.boundaries[i].information_fraction;
            let cum = spending_function_alpha(spending, t, golden.overall_alpha);
            golden.boundaries[i].cumulative_alpha = cum;
            golden.boundaries[i].incremental_alpha = cum - prev_cum;
            golden.boundaries[i].critical_value = *crit;
            prev_cum = cum;
        }
        update_gst_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let name = &golden.test_name;
    for (i, (actual_crit, expected)) in bounds.iter().zip(golden.boundaries.iter()).enumerate() {
        // Validate spending function cumulative alpha
        let t = expected.information_fraction;
        let cum_alpha = spending_function_alpha(spending, t, golden.overall_alpha);
        assert_close(
            cum_alpha,
            expected.cumulative_alpha,
            &format!("cumulative_alpha[look={}]", i + 1),
            name,
        );

        // Validate critical value
        if expected.critical_value.is_finite() {
            assert_close(
                *actual_crit,
                expected.critical_value,
                &format!("critical_value[look={}]", i + 1),
                name,
            );
        }
    }
}

// ----- mSPRT golden tests -----

#[test]
fn golden_msprt_no_effect() {
    run_msprt_golden("msprt_no_effect.json");
}

#[test]
fn golden_msprt_moderate_effect() {
    run_msprt_golden("msprt_moderate_effect.json");
}

#[test]
fn golden_msprt_strong_effect() {
    run_msprt_golden("msprt_strong_effect.json");
}

#[test]
fn golden_msprt_small_sample() {
    run_msprt_golden("msprt_small_sample.json");
}

#[test]
fn golden_msprt_large_tau() {
    run_msprt_golden("msprt_large_tau.json");
}

// ----- GST golden tests (original 4) -----

#[test]
fn golden_gst_obf_4_looks() {
    run_gst_golden("gst_obf_4_looks.json");
}

#[test]
fn golden_gst_pocock_4_looks() {
    run_gst_golden("gst_pocock_4_looks.json");
}

#[test]
fn golden_gst_obf_5_looks_alpha10() {
    run_gst_golden("gst_obf_5_looks_alpha10.json");
}

#[test]
fn golden_gst_pocock_3_looks() {
    run_gst_golden("gst_pocock_3_looks.json");
}

// ----- GST golden tests (new 6) -----

#[test]
fn golden_gst_obf_2_looks() {
    run_gst_golden("gst_obf_2_looks.json");
}

#[test]
fn golden_gst_pocock_2_looks() {
    run_gst_golden("gst_pocock_2_looks.json");
}

#[test]
fn golden_gst_obf_6_looks() {
    run_gst_golden("gst_obf_6_looks.json");
}

#[test]
fn golden_gst_pocock_6_looks() {
    run_gst_golden("gst_pocock_6_looks.json");
}

#[test]
fn golden_gst_obf_3_looks_alpha01() {
    run_gst_golden("gst_obf_3_looks_alpha01.json");
}

#[test]
fn golden_gst_pocock_5_looks_alpha10() {
    run_gst_golden("gst_pocock_5_looks_alpha10.json");
}

// ----- Invariant tests -----

/// OBF spending function should spend very little alpha early.
#[test]
fn obf_spending_conservative_early() {
    let early = spending_function_alpha(SpendingFunction::OBrienFleming, 0.25, 0.05);
    assert!(
        early < 0.001,
        "OBF should spend < 0.1% alpha at 25% information, got {early:.6}"
    );
}

/// Spending function at t=1.0 should equal overall alpha.
#[test]
fn spending_function_exhausts_alpha() {
    for sf in [SpendingFunction::OBrienFleming, SpendingFunction::Pocock] {
        let final_alpha = spending_function_alpha(sf, 1.0, 0.05);
        assert!(
            (final_alpha - 0.05).abs() < 1e-6,
            "{sf:?} at t=1.0 should equal 0.05, got {final_alpha}"
        );
    }
}

/// OBF boundaries should be monotonically decreasing.
#[test]
fn obf_boundaries_decreasing() {
    let bounds = gst_boundaries(5, 0.05, SpendingFunction::OBrienFleming).unwrap();
    for i in 1..bounds.len() {
        assert!(
            bounds[i] <= bounds[i - 1] + 1e-10,
            "OBF boundary at look {} ({}) should be <= look {} ({})",
            i + 1,
            bounds[i],
            i,
            bounds[i - 1]
        );
    }
}
