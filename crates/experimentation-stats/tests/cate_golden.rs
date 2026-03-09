//! Golden-file integration tests for CATE (Conditional Average Treatment Effects).
//!
//! Expected values computed via R's `t.test(..., var.equal=FALSE)`, `p.adjust(method="BH")`,
//! and manual Cochran Q / I² calculations.
//!
//! Run: cargo test -p experimentation-stats --test cate_golden
//! Update golden files: UPDATE_GOLDEN=1 cargo test -p experimentation-stats --test cate_golden

use experimentation_stats::cate::{analyze_cate, SubgroupInput};
use std::path::PathBuf;

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenCate {
    test_name: String,
    r_command: String,
    alpha: f64,
    fdr: f64,
    subgroups: Vec<GoldenSubgroupInput>,
    expected: GoldenExpected,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenSubgroupInput {
    segment: String,
    control: Vec<f64>,
    treatment: Vec<f64>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenExpected {
    global_ate: f64,
    global_se: f64,
    global_ci_lower: f64,
    global_ci_upper: f64,
    global_p_value: f64,
    subgroup_effects: Vec<GoldenSubgroupEffect>,
    heterogeneity: GoldenHeterogeneity,
    n_subgroups: usize,
    fdr_threshold: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenSubgroupEffect {
    segment: String,
    effect: f64,
    se: f64,
    ci_lower: f64,
    ci_upper: f64,
    p_value_raw: f64,
    p_value_adjusted: f64,
    is_significant: bool,
    n_control: usize,
    n_treatment: usize,
    control_mean: f64,
    treatment_mean: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenHeterogeneity {
    q_statistic: f64,
    df: f64,
    p_value: f64,
    i_squared: f64,
    heterogeneity_detected: bool,
}

const TOLERANCE: f64 = 1e-6;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_golden(filename: &str) -> GoldenCate {
    let path = golden_dir().join(filename);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read golden file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse golden file {}: {e}", path.display()))
}

fn update_golden(filename: &str, golden: &GoldenCate) {
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

    let subgroups: Vec<SubgroupInput> = golden
        .subgroups
        .iter()
        .map(|s| SubgroupInput {
            segment: s.segment.clone(),
            control: s.control.clone(),
            treatment: s.treatment.clone(),
        })
        .collect();

    let result = analyze_cate(&subgroups, golden.alpha, golden.fdr)
        .unwrap_or_else(|e| panic!("[{}] analyze_cate failed: {e}", golden.test_name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        golden.expected = GoldenExpected {
            global_ate: result.global_ate,
            global_se: result.global_se,
            global_ci_lower: result.global_ci_lower,
            global_ci_upper: result.global_ci_upper,
            global_p_value: result.global_p_value,
            subgroup_effects: result
                .subgroup_effects
                .iter()
                .map(|sg| GoldenSubgroupEffect {
                    segment: sg.segment.clone(),
                    effect: sg.effect,
                    se: sg.se,
                    ci_lower: sg.ci_lower,
                    ci_upper: sg.ci_upper,
                    p_value_raw: sg.p_value_raw,
                    p_value_adjusted: sg.p_value_adjusted,
                    is_significant: sg.is_significant,
                    n_control: sg.n_control,
                    n_treatment: sg.n_treatment,
                    control_mean: sg.control_mean,
                    treatment_mean: sg.treatment_mean,
                })
                .collect(),
            heterogeneity: GoldenHeterogeneity {
                q_statistic: result.heterogeneity.q_statistic,
                df: result.heterogeneity.df,
                p_value: result.heterogeneity.p_value,
                i_squared: result.heterogeneity.i_squared,
                heterogeneity_detected: result.heterogeneity.heterogeneity_detected,
            },
            n_subgroups: result.n_subgroups,
            fdr_threshold: result.fdr_threshold,
        };
        update_golden(filename, &golden);
        eprintln!("[{}] Updated golden file: {}", golden.test_name, filename);
        return;
    }

    let expected = &golden.expected;
    let name = &golden.test_name;

    // Global ATE
    assert_close(result.global_ate, expected.global_ate, "global_ate", name);
    assert_close(result.global_se, expected.global_se, "global_se", name);
    assert_close(
        result.global_ci_lower,
        expected.global_ci_lower,
        "global_ci_lower",
        name,
    );
    assert_close(
        result.global_ci_upper,
        expected.global_ci_upper,
        "global_ci_upper",
        name,
    );
    assert_close(
        result.global_p_value,
        expected.global_p_value,
        "global_p_value",
        name,
    );

    // Subgroup effects
    assert_eq!(
        result.subgroup_effects.len(),
        expected.subgroup_effects.len(),
        "[{name}] subgroup count mismatch"
    );
    for (i, (actual, exp)) in result
        .subgroup_effects
        .iter()
        .zip(expected.subgroup_effects.iter())
        .enumerate()
    {
        let label = format!("subgroup[{i}]({}).{{}}", actual.segment);
        assert_eq!(
            actual.segment, exp.segment,
            "[{name}] segment name mismatch at index {i}"
        );
        assert_close(actual.effect, exp.effect, &label.replace("{}", "effect"), name);
        assert_close(actual.se, exp.se, &label.replace("{}", "se"), name);
        assert_close(
            actual.ci_lower,
            exp.ci_lower,
            &label.replace("{}", "ci_lower"),
            name,
        );
        assert_close(
            actual.ci_upper,
            exp.ci_upper,
            &label.replace("{}", "ci_upper"),
            name,
        );
        assert_close(
            actual.p_value_raw,
            exp.p_value_raw,
            &label.replace("{}", "p_value_raw"),
            name,
        );
        assert_close(
            actual.p_value_adjusted,
            exp.p_value_adjusted,
            &label.replace("{}", "p_value_adjusted"),
            name,
        );
        assert_eq!(
            actual.is_significant, exp.is_significant,
            "[{name}] {}: expected {}, got {}",
            label.replace("{}", "is_significant"),
            exp.is_significant,
            actual.is_significant
        );
        assert_eq!(actual.n_control, exp.n_control);
        assert_eq!(actual.n_treatment, exp.n_treatment);
        assert_close(
            actual.control_mean,
            exp.control_mean,
            &label.replace("{}", "control_mean"),
            name,
        );
        assert_close(
            actual.treatment_mean,
            exp.treatment_mean,
            &label.replace("{}", "treatment_mean"),
            name,
        );
    }

    // Heterogeneity
    assert_close(
        result.heterogeneity.q_statistic,
        expected.heterogeneity.q_statistic,
        "heterogeneity.q_statistic",
        name,
    );
    assert_close(
        result.heterogeneity.df,
        expected.heterogeneity.df,
        "heterogeneity.df",
        name,
    );
    assert_close(
        result.heterogeneity.p_value,
        expected.heterogeneity.p_value,
        "heterogeneity.p_value",
        name,
    );
    assert_close(
        result.heterogeneity.i_squared,
        expected.heterogeneity.i_squared,
        "heterogeneity.i_squared",
        name,
    );
    assert_eq!(
        result.heterogeneity.heterogeneity_detected,
        expected.heterogeneity.heterogeneity_detected,
        "[{name}] heterogeneity_detected mismatch"
    );

    // Metadata
    assert_eq!(result.n_subgroups, expected.n_subgroups);
    assert!((result.fdr_threshold - expected.fdr_threshold).abs() < 1e-15);
}

#[test]
fn golden_cate_homogeneous_3_segments() {
    run_golden_test("cate_homogeneous_3_segments.json");
}

#[test]
fn golden_cate_heterogeneous_3_segments() {
    run_golden_test("cate_heterogeneous_3_segments.json");
}

#[test]
fn golden_cate_6_lifecycle_segments() {
    run_golden_test("cate_6_lifecycle_segments.json");
}

#[test]
fn golden_cate_borderline_significance() {
    run_golden_test("cate_borderline_significance.json");
}
