//! Conditional Average Treatment Effects (CATE) — heterogeneous treatment effects.
//!
//! Implements lifecycle stratification: pre-specified subgroup analysis per
//! `LifecycleSegment` with Benjamini-Hochberg FDR correction and Cochran Q
//! test for treatment effect heterogeneity.
//!
//! Validated against R's `t.test()`, `p.adjust(method="BH")`, and manual
//! Cochran Q / I² computations to 6 decimal places.

use experimentation_core::error::{assert_finite, Error, Result};
use statrs::distribution::{ChiSquared, ContinuousCDF};

use crate::multiple_comparison::benjamini_hochberg;
use crate::ttest::welch_ttest;

/// Input data for a single subgroup (lifecycle segment).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubgroupInput {
    /// Segment label, e.g. "TRIAL", "NEW", "ESTABLISHED".
    pub segment: String,
    /// Observations from the control group.
    pub control: Vec<f64>,
    /// Observations from the treatment group.
    pub treatment: Vec<f64>,
}

/// Per-subgroup treatment effect with BH-corrected significance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubgroupEffect {
    pub segment: String,
    pub effect: f64,
    pub se: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub p_value_raw: f64,
    pub p_value_adjusted: f64,
    pub is_significant: bool,
    pub n_control: usize,
    pub n_treatment: usize,
    pub control_mean: f64,
    pub treatment_mean: f64,
}

/// Cochran Q heterogeneity test result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HeterogeneityTest {
    /// Cochran Q statistic.
    pub q_statistic: f64,
    /// Degrees of freedom (K - 1).
    pub df: f64,
    /// P-value from chi-squared(K-1).
    pub p_value: f64,
    /// Percentage of variability due to heterogeneity (0–100).
    pub i_squared: f64,
    /// Whether heterogeneity is detected at the given alpha.
    pub heterogeneity_detected: bool,
}

/// Full CATE analysis result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CateResult {
    pub global_ate: f64,
    pub global_se: f64,
    pub global_ci_lower: f64,
    pub global_ci_upper: f64,
    pub global_p_value: f64,
    pub subgroup_effects: Vec<SubgroupEffect>,
    pub heterogeneity: HeterogeneityTest,
    pub n_subgroups: usize,
    pub fdr_threshold: f64,
}

/// Analyze conditional average treatment effects across lifecycle segments.
///
/// 1. Pools all samples → Welch t-test for global ATE.
/// 2. Per-subgroup: Welch t-test + SE computation.
/// 3. Benjamini-Hochberg FDR correction on subgroup p-values.
/// 4. Cochran Q test for treatment effect heterogeneity.
///
/// # Arguments
/// * `subgroups` — At least 2 subgroups, each with ≥ 2 control and ≥ 2 treatment observations.
/// * `alpha` — Significance level for heterogeneity test (e.g. 0.05).
/// * `fdr` — False discovery rate threshold for BH correction (e.g. 0.05).
///
/// # Errors
/// Returns `Error::Validation` for insufficient subgroups/samples or out-of-range alpha/fdr.
/// Panics (fail-fast) if any intermediate floating-point result is non-finite.
pub fn analyze_cate(subgroups: &[SubgroupInput], alpha: f64, fdr: f64) -> Result<CateResult> {
    // Validation
    if subgroups.len() < 2 {
        return Err(Error::Validation(
            "CATE requires at least 2 subgroups".into(),
        ));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }
    if fdr <= 0.0 || fdr >= 1.0 {
        return Err(Error::Validation("fdr must be in (0, 1)".into()));
    }
    for (i, sg) in subgroups.iter().enumerate() {
        if sg.control.len() < 2 {
            return Err(Error::Validation(format!(
                "subgroup[{i}] ({}) must have ≥ 2 control observations, got {}",
                sg.segment,
                sg.control.len()
            )));
        }
        if sg.treatment.len() < 2 {
            return Err(Error::Validation(format!(
                "subgroup[{i}] ({}) must have ≥ 2 treatment observations, got {}",
                sg.segment,
                sg.treatment.len()
            )));
        }
    }

    // Pool all samples for global ATE
    let (pooled_control, pooled_treatment) = pool_samples(subgroups);
    let global = welch_ttest(&pooled_control, &pooled_treatment, alpha)?;

    // Per-subgroup t-tests
    let mut effects = Vec::with_capacity(subgroups.len());
    let mut ses = Vec::with_capacity(subgroups.len());
    let mut raw_p_values = Vec::with_capacity(subgroups.len());
    let mut subgroup_results = Vec::with_capacity(subgroups.len());

    for sg in subgroups {
        let ttest = welch_ttest(&sg.control, &sg.treatment, alpha)?;
        let se = compute_welch_se(&sg.control, &sg.treatment);

        effects.push(ttest.effect);
        ses.push(se);
        raw_p_values.push(ttest.p_value);

        subgroup_results.push(SubgroupEffect {
            segment: sg.segment.clone(),
            effect: ttest.effect,
            se,
            ci_lower: ttest.ci_lower,
            ci_upper: ttest.ci_upper,
            p_value_raw: ttest.p_value,
            p_value_adjusted: 0.0, // filled below
            is_significant: false,  // filled below
            n_control: sg.control.len(),
            n_treatment: sg.treatment.len(),
            control_mean: ttest.control_mean,
            treatment_mean: ttest.treatment_mean,
        });
    }

    // BH correction
    let bh = benjamini_hochberg(&raw_p_values, fdr)?;
    for (i, sg_result) in subgroup_results.iter_mut().enumerate() {
        sg_result.p_value_adjusted = bh.p_values_adjusted[i];
        sg_result.is_significant = bh.rejected[i];
    }

    // Cochran Q heterogeneity test
    let heterogeneity = cochran_q_test(&effects, &ses, alpha)?;

    Ok(CateResult {
        global_ate: global.effect,
        global_se: compute_welch_se(&pooled_control, &pooled_treatment),
        global_ci_lower: global.ci_lower,
        global_ci_upper: global.ci_upper,
        global_p_value: global.p_value,
        subgroup_effects: subgroup_results,
        heterogeneity,
        n_subgroups: subgroups.len(),
        fdr_threshold: fdr,
    })
}

/// Cochran Q test for heterogeneity of treatment effects.
///
/// Tests whether treatment effects across subgroups are consistent (homogeneous)
/// or significantly different (heterogeneous).
///
/// # Arguments
/// * `effects` — Point estimates τ_k per subgroup.
/// * `standard_errors` — Standard errors se_k per subgroup.
/// * `alpha` — Significance level for heterogeneity detection.
///
/// # Returns
/// `HeterogeneityTest` with Q statistic, I², and p-value.
///
/// # Errors
/// Returns `Error::Validation` if inputs are mismatched or contain zero SEs.
pub fn cochran_q_test(
    effects: &[f64],
    standard_errors: &[f64],
    alpha: f64,
) -> Result<HeterogeneityTest> {
    if effects.len() != standard_errors.len() {
        return Err(Error::Validation(format!(
            "effects length ({}) must match standard_errors length ({})",
            effects.len(),
            standard_errors.len()
        )));
    }
    if effects.len() < 2 {
        return Err(Error::Validation(
            "Cochran Q requires at least 2 studies".into(),
        ));
    }
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    // Check for zero SEs (would produce infinite weights)
    for (i, &se) in standard_errors.iter().enumerate() {
        assert_finite(se, &format!("standard_error[{i}]"));
        if se <= 0.0 {
            return Err(Error::Validation(format!(
                "standard_error[{i}] must be > 0, got {se}"
            )));
        }
    }

    let k = effects.len() as f64;

    // Inverse-variance weights: w_k = 1 / se_k²
    let weights: Vec<f64> = standard_errors
        .iter()
        .enumerate()
        .map(|(i, &se)| {
            let w = 1.0 / (se * se);
            assert_finite(w, &format!("weight[{i}]"));
            w
        })
        .collect();

    // Weighted mean: τ̄ = Σ(w_k · τ_k) / Σ(w_k)
    let sum_w: f64 = weights.iter().sum();
    assert_finite(sum_w, "sum_weights");

    let weighted_sum: f64 = weights
        .iter()
        .zip(effects.iter())
        .map(|(&w, &e)| {
            assert_finite(e, "effect");
            w * e
        })
        .sum();
    assert_finite(weighted_sum, "weighted_sum_effects");

    let tau_bar = weighted_sum / sum_w;
    assert_finite(tau_bar, "weighted_mean_effect");

    // Q = Σ w_k · (τ_k - τ̄)²
    let q: f64 = weights
        .iter()
        .zip(effects.iter())
        .map(|(&w, &e)| {
            let dev = e - tau_bar;
            w * dev * dev
        })
        .sum();
    assert_finite(q, "Q_statistic");

    let df = k - 1.0;

    // P-value from chi-squared(K-1) distribution
    let p_value = if q == 0.0 {
        1.0
    } else {
        let chi_sq = ChiSquared::new(df)
            .map_err(|e| Error::Numerical(format!("chi-squared distribution error: {e}")))?;
        1.0 - chi_sq.cdf(q)
    };
    assert_finite(p_value, "cochran_q_p_value");

    // I² = max(0, (Q - (K-1)) / Q) × 100
    let i_squared = if q > 0.0 {
        ((q - df) / q).max(0.0) * 100.0
    } else {
        0.0
    };
    assert_finite(i_squared, "I_squared");

    Ok(HeterogeneityTest {
        q_statistic: q,
        df,
        p_value,
        i_squared,
        heterogeneity_detected: p_value < alpha,
    })
}

/// Compute Welch standard error from raw samples.
fn compute_welch_se(control: &[f64], treatment: &[f64]) -> f64 {
    let n_c = control.len() as f64;
    let n_t = treatment.len() as f64;

    let mean_c = control.iter().sum::<f64>() / n_c;
    let mean_t = treatment.iter().sum::<f64>() / n_t;

    let var_c = control.iter().map(|x| (x - mean_c).powi(2)).sum::<f64>() / (n_c - 1.0);
    let var_t = treatment.iter().map(|x| (x - mean_t).powi(2)).sum::<f64>() / (n_t - 1.0);

    let se = (var_c / n_c + var_t / n_t).sqrt();
    assert_finite(se, "welch_se");
    se
}

/// Concatenate control/treatment samples across all subgroups.
fn pool_samples(subgroups: &[SubgroupInput]) -> (Vec<f64>, Vec<f64>) {
    let total_c: usize = subgroups.iter().map(|s| s.control.len()).sum();
    let total_t: usize = subgroups.iter().map(|s| s.treatment.len()).sum();

    let mut control = Vec::with_capacity(total_c);
    let mut treatment = Vec::with_capacity(total_t);

    for sg in subgroups {
        control.extend_from_slice(&sg.control);
        treatment.extend_from_slice(&sg.treatment);
    }

    (control, treatment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_subgroup(segment: &str, control: Vec<f64>, treatment: Vec<f64>) -> SubgroupInput {
        SubgroupInput {
            segment: segment.to_string(),
            control,
            treatment,
        }
    }

    #[test]
    fn test_basic_cate_two_subgroups() {
        let subgroups = vec![
            make_subgroup(
                "TRIAL",
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
                vec![2.0, 3.0, 4.0, 5.0, 6.0],
            ),
            make_subgroup(
                "ESTABLISHED",
                vec![10.0, 11.0, 12.0, 13.0, 14.0],
                vec![12.0, 13.0, 14.0, 15.0, 16.0],
            ),
        ];
        let result = analyze_cate(&subgroups, 0.05, 0.05).unwrap();
        assert_eq!(result.n_subgroups, 2);
        assert_eq!(result.subgroup_effects.len(), 2);
        assert_eq!(result.subgroup_effects[0].segment, "TRIAL");
        assert_eq!(result.subgroup_effects[1].segment, "ESTABLISHED");
        // TRIAL: effect = 1.0, ESTABLISHED: effect = 2.0
        assert!((result.subgroup_effects[0].effect - 1.0).abs() < 1e-10);
        assert!((result.subgroup_effects[1].effect - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_homogeneous_effects() {
        // Same effect size in both groups → Q should be very small
        let subgroups = vec![
            make_subgroup(
                "A",
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
                vec![2.0, 3.0, 4.0, 5.0, 6.0],
            ),
            make_subgroup(
                "B",
                vec![10.0, 11.0, 12.0, 13.0, 14.0],
                vec![11.0, 12.0, 13.0, 14.0, 15.0],
            ),
        ];
        let result = analyze_cate(&subgroups, 0.05, 0.05).unwrap();
        // Q should be approximately 0 since effects are identical
        assert!(
            result.heterogeneity.q_statistic < 1e-10,
            "Q should be ~0 for homogeneous effects, got {}",
            result.heterogeneity.q_statistic
        );
        assert!(
            result.heterogeneity.i_squared < 1e-6,
            "I² should be ~0 for homogeneous effects"
        );
        assert!(!result.heterogeneity.heterogeneity_detected);
    }

    #[test]
    fn test_heterogeneous_effects() {
        // Very different effect sizes → Q should be large
        let subgroups = vec![
            make_subgroup(
                "A",
                vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
                vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0], // effect = 0
            ),
            make_subgroup(
                "B",
                vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
                vec![11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0], // effect = 10
            ),
        ];
        let result = analyze_cate(&subgroups, 0.05, 0.05).unwrap();
        assert!(
            result.heterogeneity.q_statistic > 10.0,
            "Q should be large for heterogeneous effects, got {}",
            result.heterogeneity.q_statistic
        );
        assert!(result.heterogeneity.heterogeneity_detected);
        assert!(result.heterogeneity.i_squared > 50.0);
    }

    #[test]
    fn test_global_ate_matches_pooled_ttest() {
        let subgroups = vec![
            make_subgroup(
                "A",
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
                vec![3.0, 4.0, 5.0, 6.0, 7.0],
            ),
            make_subgroup(
                "B",
                vec![10.0, 11.0, 12.0, 13.0, 14.0],
                vec![11.0, 12.0, 13.0, 14.0, 15.0],
            ),
        ];
        let result = analyze_cate(&subgroups, 0.05, 0.05).unwrap();

        // Pool manually and compare
        let pooled_c = vec![1.0, 2.0, 3.0, 4.0, 5.0, 10.0, 11.0, 12.0, 13.0, 14.0];
        let pooled_t = vec![3.0, 4.0, 5.0, 6.0, 7.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let direct = welch_ttest(&pooled_c, &pooled_t, 0.05).unwrap();

        assert!(
            (result.global_ate - direct.effect).abs() < 1e-10,
            "global ATE should match pooled t-test"
        );
        assert!(
            (result.global_p_value - direct.p_value).abs() < 1e-10,
            "global p-value should match pooled t-test"
        );
    }

    #[test]
    fn test_bh_correction_applied() {
        // Subgroups with marginal p-values where BH would make a difference
        let subgroups = vec![
            make_subgroup(
                "A",
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
                vec![3.0, 4.0, 5.0, 6.0, 7.0],
            ),
            make_subgroup(
                "B",
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
                vec![1.1, 2.1, 3.1, 4.1, 5.1],
            ),
        ];
        let result = analyze_cate(&subgroups, 0.05, 0.05).unwrap();

        // Adjusted p-values should be >= raw p-values
        for sg in &result.subgroup_effects {
            assert!(
                sg.p_value_adjusted >= sg.p_value_raw - 1e-15,
                "adjusted p {} should be >= raw p {}",
                sg.p_value_adjusted,
                sg.p_value_raw
            );
        }
    }

    #[test]
    fn test_validation_single_subgroup() {
        let subgroups = vec![make_subgroup(
            "A",
            vec![1.0, 2.0, 3.0],
            vec![2.0, 3.0, 4.0],
        )];
        let err = analyze_cate(&subgroups, 0.05, 0.05).unwrap_err();
        assert!(
            matches!(err, Error::Validation(_)),
            "single subgroup should be Validation error"
        );
    }

    #[test]
    fn test_validation_insufficient_samples() {
        let subgroups = vec![
            make_subgroup("A", vec![1.0], vec![2.0, 3.0, 4.0]),
            make_subgroup("B", vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 4.0]),
        ];
        let err = analyze_cate(&subgroups, 0.05, 0.05).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn test_validation_alpha_bounds() {
        let subgroups = vec![
            make_subgroup("A", vec![1.0, 2.0], vec![2.0, 3.0]),
            make_subgroup("B", vec![1.0, 2.0], vec![2.0, 3.0]),
        ];
        assert!(analyze_cate(&subgroups, 0.0, 0.05).is_err());
        assert!(analyze_cate(&subgroups, 1.0, 0.05).is_err());
        assert!(analyze_cate(&subgroups, 0.05, 0.0).is_err());
        assert!(analyze_cate(&subgroups, 0.05, 1.0).is_err());
    }

    #[test]
    fn test_cochran_q_equal_effects() {
        let effects = vec![1.0, 1.0, 1.0];
        let ses = vec![0.5, 0.5, 0.5];
        let result = cochran_q_test(&effects, &ses, 0.05).unwrap();
        assert!(
            result.q_statistic.abs() < 1e-10,
            "Q should be 0 for equal effects"
        );
        assert!(
            (result.p_value - 1.0).abs() < 1e-10,
            "p-value should be 1.0 for Q=0"
        );
        assert!(result.i_squared.abs() < 1e-10);
    }

    #[test]
    fn test_cochran_q_mismatched_lengths() {
        let err = cochran_q_test(&[1.0, 2.0], &[0.5], 0.05).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn test_cochran_q_zero_se() {
        let err = cochran_q_test(&[1.0, 2.0], &[0.5, 0.0], 0.05).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_nan_input_panics() {
        let subgroups = vec![
            make_subgroup("A", vec![1.0, f64::NAN, 3.0], vec![2.0, 3.0, 4.0]),
            make_subgroup("B", vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 4.0]),
        ];
        let _ = analyze_cate(&subgroups, 0.05, 0.05);
    }

    mod proptest_cate {
        use super::*;
        use proptest::prelude::*;

        fn finite_f64() -> impl Strategy<Value = f64> {
            (-1000.0f64..1000.0f64).prop_filter("finite", |v| v.is_finite())
        }

        fn sample_vec(min: usize, max: usize) -> impl Strategy<Value = Vec<f64>> {
            prop::collection::vec(finite_f64(), min..=max)
        }

        fn subgroup_input() -> impl Strategy<Value = SubgroupInput> {
            (sample_vec(5, 20), sample_vec(5, 20)).prop_map(|(control, treatment)| SubgroupInput {
                segment: "test".to_string(),
                control,
                treatment,
            })
        }

        fn subgroup_inputs(
            min: usize,
            max: usize,
        ) -> impl Strategy<Value = Vec<SubgroupInput>> {
            prop::collection::vec(subgroup_input(), min..=max).prop_map(|mut sgs| {
                for (i, sg) in sgs.iter_mut().enumerate() {
                    sg.segment = format!("segment_{i}");
                }
                sgs
            })
        }

        proptest! {
            #[test]
            fn all_outputs_finite(subgroups in subgroup_inputs(2, 5)) {
                if let Ok(result) = analyze_cate(&subgroups, 0.05, 0.05) {
                    prop_assert!(result.global_ate.is_finite());
                    prop_assert!(result.global_se.is_finite());
                    prop_assert!(result.global_ci_lower.is_finite());
                    prop_assert!(result.global_ci_upper.is_finite());
                    prop_assert!(result.global_p_value.is_finite());
                    prop_assert!(result.heterogeneity.q_statistic.is_finite());
                    prop_assert!(result.heterogeneity.p_value.is_finite());
                    prop_assert!(result.heterogeneity.i_squared.is_finite());
                    for sg in &result.subgroup_effects {
                        prop_assert!(sg.effect.is_finite());
                        prop_assert!(sg.se.is_finite());
                        prop_assert!(sg.ci_lower.is_finite());
                        prop_assert!(sg.ci_upper.is_finite());
                        prop_assert!(sg.p_value_raw.is_finite());
                        prop_assert!(sg.p_value_adjusted.is_finite());
                    }
                }
            }

            #[test]
            fn p_values_in_unit_interval(subgroups in subgroup_inputs(2, 5)) {
                if let Ok(result) = analyze_cate(&subgroups, 0.05, 0.05) {
                    prop_assert!(result.global_p_value >= 0.0 && result.global_p_value <= 1.0);
                    for sg in &result.subgroup_effects {
                        prop_assert!(sg.p_value_raw >= 0.0 && sg.p_value_raw <= 1.0,
                            "raw p {} out of [0,1]", sg.p_value_raw);
                        prop_assert!(sg.p_value_adjusted >= 0.0 && sg.p_value_adjusted <= 1.0,
                            "adjusted p {} out of [0,1]", sg.p_value_adjusted);
                    }
                    prop_assert!(result.heterogeneity.p_value >= 0.0
                        && result.heterogeneity.p_value <= 1.0);
                }
            }

            #[test]
            fn ci_contains_estimate(subgroups in subgroup_inputs(2, 5)) {
                if let Ok(result) = analyze_cate(&subgroups, 0.05, 0.05) {
                    for sg in &result.subgroup_effects {
                        prop_assert!(sg.ci_lower <= sg.effect + 1e-10,
                            "ci_lower {} > effect {}", sg.ci_lower, sg.effect);
                        prop_assert!(sg.ci_upper >= sg.effect - 1e-10,
                            "ci_upper {} < effect {}", sg.ci_upper, sg.effect);
                    }
                }
            }

            #[test]
            fn adjusted_p_ge_raw(subgroups in subgroup_inputs(2, 5)) {
                if let Ok(result) = analyze_cate(&subgroups, 0.05, 0.05) {
                    for sg in &result.subgroup_effects {
                        prop_assert!(sg.p_value_adjusted >= sg.p_value_raw - 1e-10,
                            "adjusted {} < raw {}", sg.p_value_adjusted, sg.p_value_raw);
                    }
                }
            }

            #[test]
            fn i_squared_in_range(subgroups in subgroup_inputs(2, 5)) {
                if let Ok(result) = analyze_cate(&subgroups, 0.05, 0.05) {
                    prop_assert!(result.heterogeneity.i_squared >= 0.0,
                        "I² {} < 0", result.heterogeneity.i_squared);
                    prop_assert!(result.heterogeneity.i_squared <= 100.0,
                        "I² {} > 100", result.heterogeneity.i_squared);
                }
            }

            #[test]
            fn q_statistic_non_negative(subgroups in subgroup_inputs(2, 5)) {
                if let Ok(result) = analyze_cate(&subgroups, 0.05, 0.05) {
                    prop_assert!(result.heterogeneity.q_statistic >= -1e-10,
                        "Q {} < 0", result.heterogeneity.q_statistic);
                }
            }
        }
    }
}
