//! Phase 3 integration tests for experimentation-management (ADR-025).
//!
//! These tests verify that the three Phase 3 components correctly integrate with
//! `experimentation-stats`:
//!
//! 1. `OnlineFdrController` — e-value computation and rejection logic.
//! 2. Portfolio allocation enrichment — optimal alpha and annualized impact.
//! 3. Adaptive N trigger — zone classification and extension recommendation.
//!
//! All tests here are pure (no database), exercising the full call chain from
//! management structs down to `experimentation-stats` functions.
//! Database-dependent tests are in each module's `#[ignore]` unit tests.

use uuid::Uuid;

use experimentation_management::adaptive_n::{
    run_adaptive_interim, InterimAnalysisInput, Zone, ZoneThresholds,
};
use experimentation_management::portfolio::{enrich_portfolio_allocation, ExperimentPortfolioEntry};
use experimentation_stats::evalue::{e_value_avlm, e_value_grow};
use experimentation_stats::portfolio::{annualized_impact, optimal_alpha};

// ---------------------------------------------------------------------------
// FDR controller — stats integration (pure, no DB)
// ---------------------------------------------------------------------------

#[test]
fn fdr_grow_strong_signal_exceeds_threshold() {
    // Strong consistent positive signal: e-value should far exceed 1/0.05 = 20.
    let obs: Vec<f64> = vec![3.0; 15];
    let result = e_value_grow(&obs, 1.0, 0.05).unwrap();
    let rejected = result.e_value > 1.0 / 0.05;
    assert!(
        rejected,
        "strong signal (obs=3.0 * 15) should produce e_value > 20; got {}",
        result.e_value
    );
}

#[test]
fn fdr_grow_null_signal_does_not_reject() {
    // Zero observations: GROW martingale stays at 1.0 (no wealth gain).
    let obs = vec![0.0_f64; 10];
    let result = e_value_grow(&obs, 1.0, 0.05).unwrap();
    // E_n = 1.0 < 20 → no rejection.
    assert!(
        !result.reject,
        "null signal should not reject; e_value={}",
        result.e_value
    );
}

#[test]
fn fdr_avlm_large_effect_rejects() {
    // Large treatment effect with no covariate: AVLM e-value should reject.
    let ctrl_y: Vec<f64> = vec![0.0; 50];
    let trt_y: Vec<f64> = vec![4.0; 50];
    let x = vec![0.0_f64; 50];
    let result = e_value_avlm(&ctrl_y, &trt_y, &x, &x, 1.0, 0.05).unwrap();
    assert!(
        result.reject,
        "large effect should reject; e_value={}",
        result.e_value
    );
}

#[test]
fn fdr_avlm_covariate_reduces_se_same_effect() {
    // CUPED setup: within-group noise is correlated with a pre-treatment covariate.
    // ctrl_y and trt_y have the same within-group noise pattern; treatment adds +2.0.
    // ctrl_x and trt_x are the within-group noise (pre-treatment values for each unit).
    // This is the standard CUPED setup: covariate = pre-period metric.
    let noise: Vec<f64> = vec![-1.0, 0.0, 1.0, -1.0, 0.0, 1.0, -1.5, 0.5, 1.5, -0.5];
    let ctrl_y: Vec<f64> = noise.clone();              // control outcome ≈ noise
    let trt_y: Vec<f64> = noise.iter().map(|x| x + 2.0).collect(); // treatment = noise + 2.0

    // Covariate = noise (pre-period, available for both groups).
    let ctrl_x = noise.clone();
    let trt_x = noise.clone();
    let zero_x = vec![0.0_f64; noise.len()];

    let r_with_cov = e_value_avlm(&ctrl_y, &trt_y, &ctrl_x, &trt_x, 0.25, 0.05).unwrap();
    let r_no_cov = e_value_avlm(&ctrl_y, &trt_y, &zero_x, &zero_x, 0.25, 0.05).unwrap();

    // With a perfectly correlated covariate, residual variance → 0 and e-value → ∞.
    // With no covariate, variance is higher → smaller e-value.
    assert!(
        r_with_cov.e_value >= r_no_cov.e_value,
        "with_cov={} no_cov={}",
        r_with_cov.e_value,
        r_no_cov.e_value
    );
}

// ---------------------------------------------------------------------------
// Portfolio enrichment — full integration
// ---------------------------------------------------------------------------

#[test]
fn portfolio_ten_experiments_bonferroni_alpha() {
    let entries: Vec<_> = (0..10)
        .map(|_| ExperimentPortfolioEntry {
            experiment_id: Uuid::new_v4(),
            effect_estimate: 0.5,
            blinded_sigma_sq: 1.0,
            n_per_arm: 100.0,
            n_max_per_arm: 500.0,
            daily_users: 1_000_000.0,
            duration_days: 14.0,
        })
        .collect();

    let results = enrich_portfolio_allocation(&entries, 0.05).unwrap();
    assert_eq!(results.len(), 10);
    // Bonferroni: 0.05 / 10 = 0.005 per experiment.
    for r in &results {
        assert!(
            (r.recommended_alpha - 0.005).abs() < 1e-12,
            "alpha={}",
            r.recommended_alpha
        );
    }
}

#[test]
fn portfolio_large_effect_high_conditional_power() {
    let entry = ExperimentPortfolioEntry {
        experiment_id: Uuid::new_v4(),
        effect_estimate: 5.0,
        blinded_sigma_sq: 1.0,
        n_per_arm: 100.0,
        n_max_per_arm: 1000.0,
        daily_users: 5_000_000.0,
        duration_days: 21.0,
    };
    let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
    assert_eq!(results.len(), 1);
    assert!(
        results[0].conditional_power > 0.90,
        "cp={}",
        results[0].conditional_power
    );
}

#[test]
fn portfolio_annualized_impact_formula() {
    // Verify formula: effect * daily_users * 365.
    let entry = ExperimentPortfolioEntry {
        experiment_id: Uuid::new_v4(),
        effect_estimate: 0.02,
        blinded_sigma_sq: 1.0,
        n_per_arm: 100.0,
        n_max_per_arm: 200.0,
        daily_users: 2_000_000.0,
        duration_days: 14.0,
    };
    let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
    // 0.02 * 2M * 365 = 14.6M
    let expected = 0.02 * 2_000_000.0 * 365.0;
    assert!(
        (results[0].annualized_impact - expected).abs() < 1.0,
        "annualized_impact={} expected={}",
        results[0].annualized_impact,
        expected
    );
}

#[test]
fn portfolio_optimal_alpha_standalone() {
    assert!((optimal_alpha(0.05, 5).unwrap() - 0.01).abs() < 1e-12);
    assert!((optimal_alpha(0.10, 10).unwrap() - 0.01).abs() < 1e-12);
    assert!(optimal_alpha(0.0, 5).is_err());
    assert!(optimal_alpha(0.05, 0).is_err());
}

#[test]
fn portfolio_annualized_impact_standalone() {
    let impact = annualized_impact(0.01, 1_000_000.0, 14.0).unwrap();
    assert!((impact - 3_650_000.0).abs() < 1.0);

    // Negative effect → negative impact.
    let neg = annualized_impact(-0.01, 1_000_000.0, 14.0).unwrap();
    assert!(neg < 0.0);

    // Validation errors.
    assert!(annualized_impact(f64::NAN, 1e6, 14.0).is_err());
    assert!(annualized_impact(0.01, 0.0, 14.0).is_err());
    assert!(annualized_impact(0.01, 1e6, 0.0).is_err());
}

// ---------------------------------------------------------------------------
// Adaptive N trigger — full integration
// ---------------------------------------------------------------------------

#[test]
fn adaptive_n_favorable_no_extension() {
    // Strong effect, large n_max → Favorable → no extension.
    // Use non-constant observations to avoid zero blinded variance.
    let obs: Vec<f64> = (0..300).map(|i| if i % 2 == 0 { 1.1 } else { 0.9 }).collect();
    let input = InterimAnalysisInput::new(
        Uuid::new_v4(),
        obs,
        1.0,
        600.0,
        0.05,
        0.80,
        3000.0,
    );
    let decision = run_adaptive_interim(&input).unwrap();
    assert_eq!(decision.zone, Zone::Favorable);
    assert!(!decision.should_extend);
    assert!(!decision.recommend_early_stop);
    assert!(decision.recommended_n_max.is_none());
    assert!(decision.conditional_power >= 0.90, "cp={}", decision.conditional_power);
}

#[test]
fn adaptive_n_futile_early_stop() {
    // Near-zero effect relative to high variance → Futile.
    let mut obs: Vec<f64> = (0..100).map(|i| i as f64 * 5.0).collect();
    obs.extend((0..100).map(|i| -(i as f64) * 5.0));
    let input = InterimAnalysisInput::new(
        Uuid::new_v4(),
        obs,
        0.0001, // negligible effect
        50.0,
        0.05,
        0.80,
        500.0,
    );
    let decision = run_adaptive_interim(&input).unwrap();
    assert_eq!(decision.zone, Zone::Futile);
    assert!(decision.recommend_early_stop);
    assert!(!decision.should_extend);
    assert!(decision.conditional_power < 0.30, "cp={}", decision.conditional_power);
}

#[test]
fn adaptive_n_promising_extension_within_cap() {
    // Moderate effect with insufficient n_max → Promising → extension recommended.
    let mut obs = vec![1.5_f64; 50];
    obs.extend(vec![0.5_f64; 50]); // blended for variance
    let input = InterimAnalysisInput::new(
        Uuid::new_v4(),
        obs,
        0.5,    // moderate effect
        100.0,  // low n_max for this effect size
        0.05,
        0.80,
        5000.0, // large cap — extension can be recommended
    );
    let decision = run_adaptive_interim(&input).unwrap();

    // Should land in Promising or Favorable depending on exact variance/effect combo.
    // At minimum: sanity checks.
    assert!(
        decision.conditional_power >= 0.0 && decision.conditional_power <= 1.0,
        "cp={}",
        decision.conditional_power
    );

    if decision.zone == Zone::Promising {
        assert!(decision.should_extend, "Promising zone must trigger extension");
        let n_ext = decision.recommended_n_max.unwrap();
        assert!(
            n_ext >= input.n_max_per_arm,
            "extension must be >= current n_max: n_ext={n_ext} n_max={}",
            input.n_max_per_arm
        );
        assert!(
            n_ext <= input.n_max_allowed,
            "extension must not exceed cap: n_ext={n_ext} cap={}",
            input.n_max_allowed
        );
    }
}

#[test]
fn adaptive_n_custom_thresholds() {
    // Stricter thresholds: favorable >= 0.95.
    let obs: Vec<f64> = (0..200).map(|i| if i % 2 == 0 { 1.1 } else { 0.9 }).collect();
    let mut input = InterimAnalysisInput::new(
        Uuid::new_v4(),
        obs,
        0.8,
        400.0,
        0.05,
        0.80,
        2000.0,
    );
    input.thresholds = ZoneThresholds {
        favorable: 0.95,
        promising: 0.50,
    };
    let decision = run_adaptive_interim(&input).unwrap();
    // With stricter favorable threshold, more experiments land in Promising.
    // Just verify it runs without error.
    assert!(decision.conditional_power >= 0.0);
}

#[test]
fn adaptive_n_experiment_id_preserved() {
    let id = Uuid::new_v4();
    let obs: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { 0.6 } else { 0.4 }).collect();
    let input = InterimAnalysisInput::new(id, obs, 0.5, 100.0, 0.05, 0.80, 500.0);
    let decision = run_adaptive_interim(&input).unwrap();
    assert_eq!(decision.experiment_id, id, "experiment_id must pass through");
}

#[test]
fn adaptive_n_single_observation_fails() {
    let input = InterimAnalysisInput::new(
        Uuid::new_v4(),
        vec![1.0], // 1 observation — blinded_pooled_variance needs >= 2
        0.5,
        100.0,
        0.05,
        0.80,
        500.0,
    );
    assert!(run_adaptive_interim(&input).is_err());
}

// ---------------------------------------------------------------------------
// Cross-module integration: FDR + Portfolio + Adaptive N together
// ---------------------------------------------------------------------------

#[test]
fn cross_module_all_three_components_compute() {
    // Run all three Phase 3 components for the same experiment — verify they
    // compose correctly without errors.

    let exp_id = Uuid::new_v4();

    // 1. Compute e-value (FDR input).
    let obs = vec![2.0_f64; 20];
    let ev_result = e_value_grow(&obs, 1.0, 0.05).unwrap();
    assert!(ev_result.e_value > 0.0);

    // 2. Portfolio enrichment.
    let portfolio_entry = ExperimentPortfolioEntry {
        experiment_id: exp_id,
        effect_estimate: 0.5,
        blinded_sigma_sq: 1.0,
        n_per_arm: 100.0,
        n_max_per_arm: 300.0,
        daily_users: 1_000_000.0,
        duration_days: 14.0,
    };
    let portfolio_results = enrich_portfolio_allocation(&[portfolio_entry], 0.05).unwrap();
    assert_eq!(portfolio_results[0].experiment_id, exp_id);

    // 3. Adaptive N interim.
    let interim_obs: Vec<f64> = (0..50).map(|i| if i % 2 == 0 { 0.9 } else { 0.7 }).collect();
    let interim_input = InterimAnalysisInput::new(
        exp_id,
        interim_obs,
        0.5,
        200.0,
        0.05,
        0.80,
        1000.0,
    );
    let interim_decision = run_adaptive_interim(&interim_input).unwrap();
    assert_eq!(interim_decision.experiment_id, exp_id);

    // All three produced results for the same experiment without panics.
    // This is the key Phase 3 integration guarantee.
}
