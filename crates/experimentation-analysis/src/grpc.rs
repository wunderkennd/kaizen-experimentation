//! gRPC server for the AnalysisService (M4a).
//!
//! All 5 RPCs are wired through Delta Lake → experimentation-stats → proto conversion.

use crate::config::AnalysisConfig;
use crate::delta_reader;
use crate::store::AnalysisStore;
use experimentation_proto::experimentation::analysis::v1::analysis_service_server::{
    AnalysisService, AnalysisServiceServer,
};
use experimentation_proto::experimentation::analysis::v1::{
    AdaptiveNInterimResult, AlgorithmStrength as ProtoAlgorithmStrength, AnalysisResult,
    GetAnalysisResultRequest, GetInterferenceAnalysisRequest, GetInterleavingAnalysisRequest,
    GetNoveltyAnalysisRequest, GetOrlAnalysisRequest, GetPortfolioAllocationRequest,
    GetPortfolioAllocationResponse, GetPortfolioPowerAnalysisRequest, GetSwitchbackAnalysisRequest,
    GetSyntheticControlAnalysisRequest, InterferenceAnalysisResult, InterleavingAnalysisResult,
    IpwResult as ProtoIpwResult, MetricResult, NoveltyAnalysisResult,
    PositionAnalysis as ProtoPositionAnalysis, PortfolioImpactParams, PortfolioPowerAnalysisResult,
    OrlAnalysisResult, PortfolioTrafficAllocation, RunAnalysisRequest, SegmentResult,
    SequentialResult, SessionLevelResult, SrmResult as ProtoSrmResult, SwitchbackAnalysisResult,
    SyntheticControlAnalysisResult, TitleSpillover,
};
use experimentation_proto::experimentation::common::v1::AdaptiveSampleSizeConfig;
use experimentation_stats::{
    adaptive_n, avlm, cate, clustering, cuped, evalue, interference, interleaving, ipw, novelty, portfolio, srm,
    switchback, synthetic_control, ttest,
};

/// Proto enum value for SEQUENTIAL_METHOD_AVLM (ADR-015).
const SEQUENTIAL_METHOD_AVLM: i32 = 4;
/// Proto enum value for SEQUENTIAL_METHOD_MSPRT (ADR-018 e-value companion).
const SEQUENTIAL_METHOD_MSPRT: i32 = 1;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

/// gRPC handler for the analysis service.
#[derive(Clone)]
pub struct AnalysisServiceHandler {
    config: AnalysisConfig,
    store: Option<Arc<AnalysisStore>>,
}

impl AnalysisServiceHandler {
    pub fn new(config: AnalysisConfig, store: Option<Arc<AnalysisStore>>) -> Self {
        Self { config, store }
    }
}

fn try_parse_uuid(id: &str) -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(id).ok()
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn now_timestamp() -> prost_types::Timestamp {
    let now = chrono::Utc::now();
    prost_types::Timestamp {
        seconds: now.timestamp(),
        nanos: now.timestamp_subsec_nanos() as i32,
    }
}

fn map_reader_error(e: anyhow::Error) -> Status {
    let msg = e.to_string();
    if msg.contains("not found") || msg.contains("data found") {
        Status::not_found(msg)
    } else if msg.contains("only") && msg.contains("variant") {
        Status::failed_precondition(msg)
    } else {
        Status::internal(msg)
    }
}

fn map_stats_error(e: impl std::fmt::Display) -> Status {
    let msg = e.to_string();
    if msg.contains("must") || msg.contains("need at least") || msg.contains("alpha") {
        Status::failed_precondition(msg)
    } else {
        Status::internal(format!("analysis failed: {msg}"))
    }
}

/// Map lifecycle segment string (from Delta Lake) to proto enum value.
fn lifecycle_segment_to_proto(s: &str) -> i32 {
    match s {
        "TRIAL" => 1,
        "NEW" => 2,
        "ESTABLISHED" => 3,
        "MATURE" => 4,
        "AT_RISK" => 5,
        "WINBACK" => 6,
        _ => 0, // UNSPECIFIED
    }
}

// ---------------------------------------------------------------------------
// Proto converters
// ---------------------------------------------------------------------------

fn to_proto_interference_result(
    experiment_id: &str,
    result: &interference::InterferenceAnalysisResult,
) -> InterferenceAnalysisResult {
    InterferenceAnalysisResult {
        experiment_id: experiment_id.to_string(),
        interference_detected: result.interference_detected,
        jensen_shannon_divergence: result.jensen_shannon_divergence,
        jaccard_similarity_top_100: result.jaccard_similarity_top_100,
        treatment_gini_coefficient: result.treatment_gini_coefficient,
        control_gini_coefficient: result.control_gini_coefficient,
        treatment_catalog_coverage: result.treatment_catalog_coverage,
        control_catalog_coverage: result.control_catalog_coverage,
        spillover_titles: result
            .spillover_titles
            .iter()
            .map(|s| TitleSpillover {
                content_id: s.content_id.clone(),
                treatment_watch_rate: s.treatment_watch_rate,
                control_watch_rate: s.control_watch_rate,
                p_value: s.p_value,
            })
            .collect(),
        computed_at: Some(now_timestamp()),
        // ADR-021 feedback-loop fields — populated by future feedback-loop analysis
        feedback_loop_detected: false,
        feedback_loop_bias_estimate: 0.0,
        contamination_effect_correlation: 0.0,
        feedback_loop_computed_at: None,
    }
}

fn to_proto_srm_result(srm: &srm::SrmResult) -> ProtoSrmResult {
    let total: u64 = srm.observed.values().sum();
    let total_f = total as f64;

    ProtoSrmResult {
        chi_squared: srm.chi_squared,
        p_value: srm.p_value,
        is_mismatch: srm.is_mismatch,
        observed_counts: srm
            .observed
            .iter()
            .map(|(k, &v)| (k.clone(), v as i64))
            .collect(),
        expected_counts: srm
            .expected
            .iter()
            .map(|(k, &frac)| (k.clone(), (frac * total_f).round() as i64))
            .collect(),
    }
}

fn to_proto_interleaving_result(
    experiment_id: &str,
    result: &interleaving::InterleavingAnalysisResult,
) -> InterleavingAnalysisResult {
    InterleavingAnalysisResult {
        experiment_id: experiment_id.to_string(),
        algorithm_win_rates: result.algorithm_win_rates.clone(),
        sign_test_p_value: result.sign_test_p_value,
        algorithm_strengths: result
            .algorithm_strengths
            .iter()
            .map(|s| ProtoAlgorithmStrength {
                algorithm_id: s.algorithm_id.clone(),
                strength: s.strength,
                ci_lower: s.ci_lower,
                ci_upper: s.ci_upper,
            })
            .collect(),
        position_analyses: result
            .position_analyses
            .iter()
            .map(|p| ProtoPositionAnalysis {
                position: p.position as i32,
                algorithm_engagement_rates: p.algorithm_engagement_rates.clone(),
            })
            .collect(),
        computed_at: Some(now_timestamp()),
    }
}

fn to_proto_novelty_result(
    experiment_id: &str,
    metric_id: &str,
    result: &novelty::NoveltyAnalysisResult,
) -> NoveltyAnalysisResult {
    NoveltyAnalysisResult {
        experiment_id: experiment_id.to_string(),
        metric_id: metric_id.to_string(),
        novelty_detected: result.novelty_detected,
        raw_treatment_effect: result.raw_treatment_effect,
        projected_steady_state_effect: result.projected_steady_state_effect,
        novelty_amplitude: result.novelty_amplitude,
        decay_constant_days: result.decay_constant_days,
        is_stabilized: result.is_stabilized,
        days_until_projected_stability: result.days_until_projected_stability,
        computed_at: Some(now_timestamp()),
    }
}

// ---------------------------------------------------------------------------
// Core analysis computation (shared by run_analysis and get_analysis_result)
// ---------------------------------------------------------------------------

async fn compute_analysis(
    config: &AnalysisConfig,
    experiment_id: &str,
    sequential_method: i32,
    tau_sq: f64,
    cuped_covariate_metric_id: &str,
    adaptive_config: Option<&AdaptiveSampleSizeConfig>,
) -> Result<AnalysisResult, Status> {
    let data = delta_reader::read_metric_summaries(&config.delta_lake_path, experiment_id)
        .await
        .map_err(map_reader_error)?;

    // Identify control variant: one named "control", or first alphabetically.
    let control_variant = if data.variant_user_counts.contains_key("control") {
        "control".to_string()
    } else {
        let mut variants: Vec<&String> = data.variant_user_counts.keys().collect();
        variants.sort();
        variants[0].clone()
    };

    // SRM check: uniform expected fractions.
    let n_variants = data.variant_user_counts.len() as f64;
    let expected_fractions: HashMap<String, f64> = data
        .variant_user_counts
        .keys()
        .map(|k| (k.clone(), 1.0 / n_variants))
        .collect();

    let srm_result = srm::srm_check(&data.variant_user_counts, &expected_fractions, 0.001)
        .map_err(map_stats_error)?;

    let alpha = config.default_alpha;

    // Per-metric, per-treatment-variant analysis.
    let mut metric_results = Vec::new();

    let mut metric_ids: Vec<&String> = data.metrics.keys().collect();
    metric_ids.sort();

    for metric_id in metric_ids {
        let variant_data = &data.metrics[metric_id];

        let control_tuples = match variant_data.get(&control_variant) {
            Some(v) => v,
            None => continue,
        };

        let control_values: Vec<f64> = control_tuples.iter().map(|(v, _)| *v).collect();

        let mut variant_ids: Vec<&String> = variant_data.keys().collect();
        variant_ids.sort();

        for variant_id in variant_ids {
            if *variant_id == control_variant {
                continue;
            }

            let treatment_tuples = &variant_data[variant_id];
            let treatment_values: Vec<f64> = treatment_tuples.iter().map(|(v, _)| *v).collect();

            let ttest_result = ttest::welch_ttest(&control_values, &treatment_values, alpha)
                .map_err(map_stats_error)?;

            let relative_effect = if ttest_result.control_mean.abs() > 1e-10 {
                ttest_result.effect / ttest_result.control_mean.abs()
            } else {
                0.0
            };

            // AVLM (ADR-015) or CUPED depending on sequential_method.
            let control_covs: Vec<f64> = control_tuples.iter().filter_map(|(_, c)| *c).collect();
            let treatment_covs: Vec<f64> =
                treatment_tuples.iter().filter_map(|(_, c)| *c).collect();

            let (cuped_effect, cuped_ci_lower, cuped_ci_upper, variance_reduction_pct, avlm_sequential) =
                if sequential_method == SEQUENTIAL_METHOD_AVLM {
                    // AVLM: anytime-valid regression-adjusted CI (ADR-015).
                    // Uses covariates when cuped_covariate_metric_id is set; falls back to
                    // mSPRT (x=0) when empty (var_x_pool == 0 triggers unadjusted path).
                    let effective_tau_sq = if tau_sq > 0.0 { tau_sq } else { config.default_tau_sq };
                    let use_covariate = !cuped_covariate_metric_id.is_empty();
                    match compute_avlm_result(control_tuples, treatment_tuples, effective_tau_sq, alpha, use_covariate) {
                        Ok(Some(av)) => {
                            let seq = SequentialResult {
                                boundary_crossed: av.is_significant,
                                alpha_spent: if av.is_significant { alpha } else { 0.0 },
                                alpha_remaining: if av.is_significant { 0.0 } else { alpha },
                                current_look: 1,
                                adjusted_p_value: 0.0,
                            };
                            (av.adjusted_effect, av.ci_lower, av.ci_upper, av.variance_reduction * 100.0, Some(seq))
                        }
                        Ok(None) => (0.0, 0.0, 0.0, 0.0, None),
                        Err(e) => {
                            warn!(metric_id, error = e, "AVLM failed, skipping sequential result");
                            (0.0, 0.0, 0.0, 0.0, None)
                        }
                    }
                } else if control_covs.len() == control_values.len()
                    && treatment_covs.len() == treatment_values.len()
                    && control_covs.len() >= 2
                    && treatment_covs.len() >= 2
                {
                    match cuped::cuped_adjust(
                        &control_values,
                        &treatment_values,
                        &control_covs,
                        &treatment_covs,
                        alpha,
                    ) {
                        Ok(cr) => (
                            cr.adjusted_effect,
                            cr.ci_lower,
                            cr.ci_upper,
                            cr.variance_reduction * 100.0,
                            None,
                        ),
                        Err(_) => (0.0, 0.0, 0.0, 0.0, None),
                    }
                } else {
                    (0.0, 0.0, 0.0, 0.0, None)
                };

            // CATE: per-segment results if lifecycle_segment data exists.
            let segment_results =
                compute_segment_results(&data, metric_id, &control_variant, variant_id, alpha);

            // ADR-018 e-value: compute GROW martingale alongside mSPRT.
            // When sequential_method == MSPRT, we observe treatment outcomes centred
            // on the control mean. σ² is estimated from the treatment sample.
            let (ev, log_ev) = if sequential_method == SEQUENTIAL_METHOD_MSPRT
                && treatment_values.len() >= 2
            {
                let control_mean = ttest_result.control_mean;
                let obs: Vec<f64> = treatment_values.iter().map(|&v| v - control_mean).collect();
                let var_t = treatment_values.iter().map(|&v| (v - ttest_result.treatment_mean).powi(2)).sum::<f64>()
                    / (treatment_values.len() as f64 - 1.0);
                let sigma_sq = if var_t > f64::EPSILON { var_t } else { 1.0 };
                match evalue::e_value_grow(&obs, sigma_sq, alpha) {
                    Ok(r) => (r.e_value, r.log_e_value),
                    Err(e) => {
                        warn!(metric_id, error = %e, "GROW e-value failed, returning 0");
                        (0.0, 0.0)
                    }
                }
            } else {
                (0.0, 0.0)
            };

            metric_results.push(MetricResult {
                metric_id: metric_id.clone(),
                variant_id: variant_id.clone(),
                control_mean: ttest_result.control_mean,
                treatment_mean: ttest_result.treatment_mean,
                absolute_effect: ttest_result.effect,
                relative_effect,
                ci_lower: ttest_result.ci_lower,
                ci_upper: ttest_result.ci_upper,
                p_value: ttest_result.p_value,
                is_significant: ttest_result.is_significant,
                cuped_adjusted_effect: cuped_effect,
                cuped_ci_lower,
                cuped_ci_upper,
                variance_reduction_pct,
                sequential_result: avlm_sequential,
                segment_results,
                session_level_result: compute_session_level_result(
                    &data,
                    metric_id,
                    &control_variant,
                    variant_id,
                    alpha,
                ),
                ipw_result: compute_ipw_result(
                    &data,
                    metric_id,
                    &control_variant,
                    variant_id,
                    alpha,
                ),
                e_value: ev,
                log_e_value: log_ev,
            });
        }
    }

    // Experiment-level Cochran Q: use smallest p-value across all metrics
    // (most significant heterogeneity signal).
    let cochran_q_p_value = compute_experiment_cochran_q(&data, &control_variant, alpha);

    // ADR-020: Adaptive sample size interim analysis.
    // Uses the first metric (alphabetical) with data for both control and treatment.
    let adaptive_n_result =
        compute_adaptive_n_result(&data, &control_variant, &metric_results, alpha, adaptive_config);

    Ok(AnalysisResult {
        experiment_id: experiment_id.to_string(),
        metric_results,
        srm_result: Some(to_proto_srm_result(&srm_result)),
        surrogate_projections: vec![],
        cochran_q_p_value,
        computed_at: Some(now_timestamp()),
        adaptive_n_result,
    })
}

// ---------------------------------------------------------------------------
// Session-level clustering helper
// ---------------------------------------------------------------------------

/// Compute HC1 clustered standard errors if session-level data is available.
///
/// Returns `Some(SessionLevelResult)` when `session_data` contains observations
/// for this metric, or `None` otherwise.
fn compute_session_level_result(
    data: &delta_reader::ExperimentMetrics,
    metric_id: &str,
    control_variant: &str,
    treatment_variant: &str,
    alpha: f64,
) -> Option<SessionLevelResult> {
    let session_map = data.session_data.get(metric_id)?;

    // Build ClusteredObservation vec from session data.
    let mut observations = Vec::new();
    for (value, user_id, variant_id) in session_map {
        let is_treatment = if variant_id == treatment_variant {
            true
        } else if variant_id == control_variant {
            false
        } else {
            continue; // skip other variants
        };
        observations.push(clustering::ClusteredObservation {
            value: *value,
            cluster_id: user_id.clone(),
            is_treatment,
        });
    }

    if observations.len() < 3 {
        return None;
    }

    match clustering::clustered_se(&observations, alpha) {
        Ok(result) => Some(SessionLevelResult {
            naive_se: result.naive_se,
            clustered_se: result.clustered_se,
            design_effect: result.design_effect,
            naive_p_value: result.naive_p_value,
            clustered_p_value: result.clustered_p_value,
        }),
        Err(e) => {
            warn!(
                metric_id = metric_id,
                error = %e,
                "failed to compute clustered SE, skipping session_level_result"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// IPW-adjusted analysis helper
// ---------------------------------------------------------------------------

/// Compute IPW-adjusted treatment effect if assignment_probability data is available.
///
/// Returns `Some(ProtoIpwResult)` when `ipw_data` contains observations
/// for this metric with both control and treatment arms, or `None` otherwise.
fn compute_ipw_result(
    data: &delta_reader::ExperimentMetrics,
    metric_id: &str,
    control_variant: &str,
    treatment_variant: &str,
    alpha: f64,
) -> Option<ProtoIpwResult> {
    let ipw_rows = data.ipw_data.get(metric_id)?;

    let observations: Vec<ipw::IpwObservation> = ipw_rows
        .iter()
        .filter_map(|(value, variant_id, prob)| {
            let is_treatment = if variant_id == treatment_variant {
                true
            } else if variant_id == control_variant {
                false
            } else {
                return None; // skip other variants
            };
            Some(ipw::IpwObservation {
                outcome: *value,
                is_treatment,
                assignment_probability: *prob,
            })
        })
        .collect();

    if observations.len() < 2 {
        return None;
    }

    match ipw::ipw_estimate(&observations, alpha, 0.01) {
        Ok(result) => Some(ProtoIpwResult {
            effect: result.effect,
            se: result.se,
            ci_lower: result.ci_lower,
            ci_upper: result.ci_upper,
            p_value: result.p_value,
            n_clipped: result.n_clipped as i32,
            effective_sample_size: result.effective_sample_size,
        }),
        Err(e) => {
            warn!(
                metric_id = metric_id,
                error = %e,
                "failed to compute IPW-adjusted effect, skipping ipw_result"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// CATE segment analysis helpers
// ---------------------------------------------------------------------------

/// Build per-segment `SegmentResult` entries for a single metric + variant pair.
///
/// Returns an empty vec if there aren't enough segments (< 2) or if the
/// metric has no segment data.
fn compute_segment_results(
    data: &delta_reader::ExperimentMetrics,
    metric_id: &str,
    control_variant: &str,
    treatment_variant: &str,
    alpha: f64,
) -> Vec<SegmentResult> {
    let metric_segments = match data.segment_data.get(metric_id) {
        Some(s) => s,
        None => return vec![],
    };

    // Build SubgroupInputs: one per segment that has both control and treatment data.
    let mut subgroups = Vec::new();
    let mut segment_names = Vec::new();

    let mut sorted_segments: Vec<&String> = metric_segments.keys().collect();
    sorted_segments.sort();

    for segment in sorted_segments {
        let variants = &metric_segments[segment];
        let control = variants.get(control_variant);
        let treatment = variants.get(treatment_variant);

        if let (Some(c), Some(t)) = (control, treatment) {
            if c.len() >= 2 && t.len() >= 2 {
                subgroups.push(cate::SubgroupInput {
                    segment: segment.clone(),
                    control: c.clone(),
                    treatment: t.clone(),
                });
                segment_names.push(segment.clone());
            }
        }
    }

    if subgroups.len() < 2 {
        return vec![];
    }

    match cate::analyze_cate(&subgroups, alpha, alpha) {
        Ok(result) => result
            .subgroup_effects
            .iter()
            .map(|sg| SegmentResult {
                segment: lifecycle_segment_to_proto(&sg.segment),
                effect: sg.effect,
                ci_lower: sg.ci_lower,
                ci_upper: sg.ci_upper,
                p_value: sg.p_value_adjusted,
                sample_size: (sg.n_control + sg.n_treatment) as i64,
            })
            .collect(),
        Err(e) => {
            warn!(metric_id, error = %e, "CATE analysis failed for metric, skipping segments");
            vec![]
        }
    }
}

/// Compute experiment-level Cochran Q p-value across all metrics.
///
/// For each metric that has segment data, runs CATE and collects the
/// heterogeneity p-value. Returns the minimum (most significant) across
/// all metrics, or 0.0 if no segment data exists.
fn compute_experiment_cochran_q(
    data: &delta_reader::ExperimentMetrics,
    control_variant: &str,
    alpha: f64,
) -> f64 {
    if data.segment_data.is_empty() {
        return 0.0;
    }

    let mut min_p = f64::MAX;
    let mut found_any = false;

    let mut sorted_metrics: Vec<&String> = data.segment_data.keys().collect();
    sorted_metrics.sort();

    for metric_id in sorted_metrics {
        let segments = &data.segment_data[metric_id];

        // For each treatment variant (non-control), try CATE.
        let mut variant_ids: Vec<&String> = data
            .metrics
            .get(metric_id)
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        variant_ids.sort();

        for variant_id in variant_ids {
            if *variant_id == control_variant {
                continue;
            }

            let mut subgroups = Vec::new();
            let mut sorted_segments: Vec<&String> = segments.keys().collect();
            sorted_segments.sort();

            for segment in sorted_segments {
                let variants = &segments[segment];
                let control = variants.get(control_variant);
                let treatment = variants.get(variant_id.as_str());

                if let (Some(c), Some(t)) = (control, treatment) {
                    if c.len() >= 2 && t.len() >= 2 {
                        subgroups.push(cate::SubgroupInput {
                            segment: segment.clone(),
                            control: c.clone(),
                            treatment: t.clone(),
                        });
                    }
                }
            }

            if subgroups.len() >= 2 {
                if let Ok(result) = cate::analyze_cate(&subgroups, alpha, alpha) {
                    found_any = true;
                    if result.heterogeneity.p_value < min_p {
                        min_p = result.heterogeneity.p_value;
                    }
                }
            }
        }
    }

    if found_any {
        min_p
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// AVLM analysis helper (ADR-015)
// ---------------------------------------------------------------------------

/// Run AVLM sequential test by streaming all observations from both arms.
///
/// When `use_covariate` is `false`, passes `x=0` for all observations, which causes
/// `AvlmSequentialTest` to fall back to the unadjusted mSPRT confidence sequence
/// (since `var_x_pool == 0` triggers `unadjusted_confidence_sequence()`).
/// When `use_covariate` is `true`, uses the `cuped_covariate` column from the data;
/// observations with `None` covariate still receive `x=0`.
fn compute_avlm_result(
    control_tuples: &[(f64, Option<f64>)],
    treatment_tuples: &[(f64, Option<f64>)],
    tau_sq: f64,
    alpha: f64,
    use_covariate: bool,
) -> Result<Option<avlm::AvlmResult>, String> {
    let mut test = avlm::AvlmSequentialTest::new(tau_sq, alpha).map_err(|e| e.to_string())?;
    for &(y, cov) in control_tuples {
        let x = if use_covariate { cov.unwrap_or(0.0) } else { 0.0 };
        test.update(y, x, false).map_err(|e| e.to_string())?;
    }
    for &(y, cov) in treatment_tuples {
        let x = if use_covariate { cov.unwrap_or(0.0) } else { 0.0 };
        test.update(y, x, true).map_err(|e| e.to_string())?;
    }
    test.confidence_sequence().map_err(|e| e.to_string())
}

/// Compute adaptive sample size interim result (ADR-020).
///
/// Uses the first metric (alphabetical) with sufficient data for both control and treatment.
/// When `adaptive_config` is `None`, returns `None` (no adaptive design configured).
fn compute_adaptive_n_result(
    data: &delta_reader::ExperimentMetrics,
    control_variant: &str,
    metric_results: &[MetricResult],
    alpha: f64,
    adaptive_config: Option<&AdaptiveSampleSizeConfig>,
) -> Option<AdaptiveNInterimResult> {
    let cfg = adaptive_config?;

    // Find the first metric alphabetically with both control and treatment data.
    let mut sorted_metrics: Vec<&String> = data.metrics.keys().collect();
    sorted_metrics.sort();

    for metric_id in sorted_metrics {
        let variant_data = &data.metrics[metric_id];

        let control_vals = match variant_data.get(control_variant) {
            Some(v) => v,
            None => continue,
        };
        let mut sorted_variants: Vec<&String> = variant_data.keys().collect();
        sorted_variants.sort();
        let treatment_vals = match sorted_variants
            .iter()
            .find(|k| k.as_str() != control_variant)
            .and_then(|k| variant_data.get(*k))
        {
            Some(v) => v,
            None => continue,
        };

        if control_vals.len() < 2 || treatment_vals.len() < 2 {
            continue;
        }

        // Gather all observations (both arms) for the blinded variance estimator.
        let mut all_obs: Vec<f64> = control_vals.iter().map(|(y, _)| *y).collect();
        all_obs.extend(treatment_vals.iter().map(|(y, _)| *y));

        // Observed effect from the already-computed metric_results.
        let observed_effect = metric_results
            .iter()
            .find(|mr| mr.metric_id == *metric_id)
            .map(|mr| mr.absolute_effect)
            .unwrap_or(0.0);

        // Infer n_max_per_arm from current sample size and interim_fraction.
        let n_current_per_arm = control_vals.len().min(treatment_vals.len()) as f64;
        let interim_fraction = if cfg.interim_fraction > 0.0 {
            cfg.interim_fraction
        } else {
            0.5
        };
        let n_max_per_arm = n_current_per_arm / interim_fraction;

        // Zone thresholds from config (with Mehta & Pocock defaults).
        let thresholds = adaptive_n::ZoneThresholds {
            favorable: if cfg.favorable_zone_lower > 0.0 {
                cfg.favorable_zone_lower
            } else {
                0.90
            },
            promising: if cfg.promising_zone_lower > 0.0 {
                cfg.promising_zone_lower
            } else {
                0.30
            },
        };
        let max_extension = if cfg.max_extension_factor > 0.0 {
            cfg.max_extension_factor
        } else {
            2.0
        };
        let n_max_allowed = n_max_per_arm * max_extension;

        match adaptive_n::run_interim_analysis(
            &all_obs,
            observed_effect,
            n_max_per_arm,
            alpha,
            &thresholds,
            0.80, // target power for Promising zone extension
            n_max_allowed,
        ) {
            Ok(result) => {
                return Some(AdaptiveNInterimResult {
                    zone: result.zone.to_string(),
                    conditional_power: result.conditional_power,
                    recommended_n_per_arm: result.recommended_n_max.unwrap_or(0.0),
                    blinded_variance: result.blinded_variance,
                });
            }
            Err(e) => {
                warn!(metric_id = metric_id.as_str(), error = %e, "adaptive_n interim analysis failed, skipping");
                continue;
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// gRPC trait implementation
// ---------------------------------------------------------------------------

#[tonic::async_trait]
impl AnalysisService for AnalysisServiceHandler {
    async fn run_analysis(
        &self,
        request: Request<RunAnalysisRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        let req = request.into_inner();
        let experiment_id = req.experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let result = compute_analysis(
            &self.config,
            &experiment_id,
            req.sequential_method,
            req.tau_sq,
            &req.cuped_covariate_metric_id,
            req.adaptive_sample_size_config.as_ref(),
        )
        .await?;

        // Fire-and-forget cache write.
        if let (Some(store), Some(uuid)) = (&self.store, try_parse_uuid(&experiment_id)) {
            if let Err(e) = store.save_analysis_result(&uuid, &result).await {
                warn!(experiment_id, error = %e, "failed to cache analysis result");
            }
        }

        Ok(Response::new(result))
    }

    async fn get_analysis_result(
        &self,
        request: Request<GetAnalysisResultRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        // Cache-first: try PostgreSQL.
        if let (Some(store), Some(uuid)) = (&self.store, try_parse_uuid(&experiment_id)) {
            match store.get_analysis_result(&uuid).await {
                Ok(Some(cached)) => return Ok(Response::new(cached)),
                Ok(None) => {} // cache miss — fall through to Delta Lake
                Err(e) => {
                    warn!(experiment_id, error = %e, "cache read failed, falling back to Delta Lake");
                }
            }
        }

        // Cache miss or no store: compute from Delta Lake (fixed-horizon, no sequential method).
        let result = compute_analysis(&self.config, &experiment_id, 0, 0.0, "", None).await?;

        // Write through to cache.
        if let (Some(store), Some(uuid)) = (&self.store, try_parse_uuid(&experiment_id)) {
            if let Err(e) = store.save_analysis_result(&uuid, &result).await {
                warn!(experiment_id, error = %e, "failed to cache analysis result");
            }
        }

        Ok(Response::new(result))
    }

    async fn get_interleaving_analysis(
        &self,
        request: Request<GetInterleavingAnalysisRequest>,
    ) -> Result<Response<InterleavingAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let scores =
            delta_reader::read_interleaving_scores(&self.config.delta_lake_path, &experiment_id)
                .await
                .map_err(map_reader_error)?;

        let result = interleaving::analyze_interleaving(&scores, self.config.default_alpha)
            .map_err(map_stats_error)?;

        Ok(Response::new(to_proto_interleaving_result(
            &experiment_id,
            &result,
        )))
    }

    async fn get_novelty_analysis(
        &self,
        request: Request<GetNoveltyAnalysisRequest>,
    ) -> Result<Response<NoveltyAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let (metric_id, effects) = delta_reader::read_daily_treatment_effects(
            &self.config.delta_lake_path,
            &experiment_id,
        )
        .await
        .map_err(map_reader_error)?;

        let result = novelty::analyze_novelty(&effects, self.config.default_alpha)
            .map_err(map_stats_error)?;

        let proto_result = to_proto_novelty_result(&experiment_id, &metric_id, &result);

        // Fire-and-forget cache write.
        if let (Some(store), Some(uuid)) = (&self.store, try_parse_uuid(&experiment_id)) {
            if let Err(e) = store.save_novelty_result(&uuid, &proto_result).await {
                warn!(experiment_id, error = %e, "failed to cache novelty result");
            }
        }

        Ok(Response::new(proto_result))
    }

    async fn get_interference_analysis(
        &self,
        request: Request<GetInterferenceAnalysisRequest>,
    ) -> Result<Response<InterferenceAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let input =
            delta_reader::read_content_consumption(&self.config.delta_lake_path, &experiment_id)
                .await
                .map_err(map_reader_error)?;

        let result = interference::analyze_interference(
            &input,
            self.config.default_alpha,
            self.config.default_js_threshold,
        )
        .map_err(|e| Status::internal(format!("analysis failed: {e}")))?;

        let proto_result = to_proto_interference_result(&experiment_id, &result);

        // Fire-and-forget cache write.
        if let (Some(store), Some(uuid)) = (&self.store, try_parse_uuid(&experiment_id)) {
            if let Err(e) = store.save_interference_result(&uuid, &proto_result).await {
                warn!(experiment_id, error = %e, "failed to cache interference result");
            }
        }

        Ok(Response::new(proto_result))
    }

    async fn get_synthetic_control_analysis(
        &self,
        request: Request<GetSyntheticControlAnalysisRequest>,
    ) -> Result<Response<SyntheticControlAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let mut sc_input = delta_reader::read_synthetic_control_panel(
            &self.config.delta_lake_path,
            &experiment_id,
        )
        .await
        .map_err(map_reader_error)?;

        // Override alpha from service config.
        sc_input.alpha = self.config.default_alpha;

        let result = synthetic_control::synthetic_control(&sc_input, synthetic_control::Method::Classic)
            .map_err(|e| Status::internal(format!("synthetic control failed: {e}")))?;

        let proto_result = SyntheticControlAnalysisResult {
            experiment_id: experiment_id.clone(),
            // Classic SCM = enum value 1.
            method: 1,
            treatment_effect: result.att,
            ci_lower: result.ci_lower,
            ci_upper: result.ci_upper,
            permutation_p_value: result.placebo_p_value,
            donor_weights: result.donor_weights,
            // pre_treatment_rmspe not yet computed by stats crate; defaults to 0.0.
            pre_treatment_rmspe: 0.0,
            computed_at: Some(now_timestamp()),
        };

        Ok(Response::new(proto_result))
    }

    async fn get_switchback_analysis(
        &self,
        request: Request<GetSwitchbackAnalysisRequest>,
    ) -> Result<Response<SwitchbackAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let periods = delta_reader::read_switchback_periods(
            &self.config.delta_lake_path,
            &experiment_id,
        )
        .await
        .map_err(map_reader_error)?;

        let analyzer = switchback::SwitchbackAnalyzer::new(periods)
            .map_err(|e| Status::internal(format!("switchback initialization failed: {e}")))?;
        let result = analyzer
            .analyze(self.config.default_alpha, 10_000, 42)
            .map_err(|e| Status::internal(format!("switchback analysis failed: {e}")))?;

        let proto_result = SwitchbackAnalysisResult {
            experiment_id: experiment_id.clone(),
            treatment_effect: result.effect,
            hac_se: result.hac_se,
            ci_lower: result.ci_lower,
            ci_upper: result.ci_upper,
            // hac_p_value not yet exposed by experimentation-stats; defaults to 0.0.
            hac_p_value: 0.0,
            ri_p_value: result.randomization_p_value,
            carryover_p_value: result.carryover_test_p_value,
            carryover_detected: result.carryover_test_p_value < self.config.default_alpha,
            computed_at: Some(now_timestamp()),
        };

        Ok(Response::new(proto_result))
    }

    async fn get_portfolio_allocation(
        &self,
        _request: Request<GetPortfolioAllocationRequest>,
    ) -> Result<Response<GetPortfolioAllocationResponse>, Status> {
        // ADR-019: Full portfolio optimization requires power-curve analysis.
        // Contract is defined; implementation is Phase 5 work.
        // The contract test exercises a test-specific implementation for wire-format validation.
        Err(Status::unimplemented(
            "GetPortfolioAllocation not yet implemented (ADR-019)",
        ))
    }

    async fn get_portfolio_power_analysis(
        &self,
        request: Request<GetPortfolioPowerAnalysisRequest>,
    ) -> Result<Response<PortfolioPowerAnalysisResult>, Status> {
        let req = request.into_inner();

        // Validate and map proto → stats types.
        let portfolio_params = portfolio::PortfolioParams {
            prior_win_rate: req.prior_win_rate,
            fdr_target: req.fdr_target,
            target_power: req.target_power,
        };

        let impact_proto = req.impact_params.unwrap_or(PortfolioImpactParams {
            observed_lift_relative: 0.0,
            annual_baseline_per_user: 0.0,
            total_users: 1,
            experiment_duration_days: 1.0,
            treatment_fraction: 0.5,
        });
        let impact_params = portfolio::AnnualizedImpactParams {
            observed_lift_relative: impact_proto.observed_lift_relative,
            annual_baseline_per_user: impact_proto.annual_baseline_per_user,
            total_users: impact_proto.total_users.max(1) as u64,
            experiment_duration_days: impact_proto.experiment_duration_days,
            treatment_fraction: impact_proto.treatment_fraction,
        };

        let experiments: Vec<portfolio::ExperimentSpec> = req
            .experiments
            .iter()
            .map(|s| portfolio::ExperimentSpec {
                experiment_id: s.experiment_id.clone(),
                mde_relative: s.mde_relative,
                baseline_mean: s.baseline_mean,
                baseline_variance: s.baseline_variance,
                n_variants: (s.n_variants.max(2)) as usize,
            })
            .collect();

        if experiments.is_empty() {
            return Err(Status::invalid_argument(
                "at least one experiment spec is required for traffic allocation",
            ));
        }

        let traffic_input = portfolio::TrafficAllocationInput {
            experiments,
            available_traffic_fraction: req.available_traffic_fraction,
            min_power: req.target_power,
            alpha: req.fdr_target, // overridden inside portfolio_power_analysis
        };

        let rec =
            portfolio::portfolio_power_analysis(&portfolio_params, &impact_params, &traffic_input)
                .map_err(map_stats_error)?;

        let proto_allocs: Vec<PortfolioTrafficAllocation> = rec
            .traffic_allocations
            .iter()
            .map(|a| PortfolioTrafficAllocation {
                experiment_id: a.experiment_id.clone(),
                recommended_traffic_fraction: a.recommended_traffic_fraction,
                required_n_per_arm: (a.required_n_per_arm.min(i64::MAX as u64)) as i64,
            })
            .collect();

        Ok(Response::new(PortfolioPowerAnalysisResult {
            optimal_alpha: rec.optimal_alpha,
            annualized_impact: rec.annualized_impact,
            traffic_allocations: proto_allocs,
            expected_portfolio_fdr: rec.expected_portfolio_fdr,
            computed_at: Some(now_timestamp()),
        }))
    }

    async fn get_orl_analysis(
        &self,
        _request: Request<GetOrlAnalysisRequest>,
    ) -> Result<Response<OrlAnalysisResult>, Status> {
        // ADR-017: Doubly-robust OPE analysis.
        // Contract is defined; full wiring is Phase 5 work.
        Err(Status::unimplemented(
            "GetOrlAnalysis not yet implemented (ADR-017)",
        ))
    }
}

/// Start the gRPC server serving the AnalysisService.
pub async fn serve_grpc(
    config: AnalysisConfig,
    store: Option<Arc<AnalysisStore>>,
) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    let handler = AnalysisServiceHandler::new(config, store);

    info!(%addr, "gRPC server starting");

    tonic::transport::Server::builder()
        .add_service(tonic_web::enable(AnalysisServiceServer::new(handler)))
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deltalake::arrow::array::{
        builder::{Float64Builder, MapBuilder, StringBuilder},
        Array, Date32Array, Float64Array, Int64Array, StringArray,
    };
    use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
    use deltalake::arrow::record_batch::RecordBatch;
    use deltalake::DeltaOps;
    use std::sync::Arc;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Test data helpers
    // -----------------------------------------------------------------------

    fn test_config(path: &str) -> AnalysisConfig {
        AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: path.into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
            database_url: None,
            default_tau_sq: 0.5,
        }
    }

    fn test_handler(path: &str) -> AnalysisServiceHandler {
        AnalysisServiceHandler::new(test_config(path), None)
    }

    async fn write_table(dir: &std::path::Path, name: &str, batch: RecordBatch) {
        let table_path = dir.join(name);
        std::fs::create_dir_all(&table_path).unwrap();
        let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
            .await
            .unwrap();
        ops.write(vec![batch]).await.unwrap();
    }

    fn metric_summaries_schema() -> Arc<ArrowSchema> {
        Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("variant_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("metric_value", DataType::Float64, false),
            Field::new("cuped_covariate", DataType::Float64, true),
        ]))
    }

    fn make_analysis_data(
        exp_ids: &[&str],
        user_ids: &[&str],
        variant_ids: &[&str],
        metric_ids: &[&str],
        values: &[f64],
        covariates: &[Option<f64>],
    ) -> RecordBatch {
        let cov_arr: Float64Array = covariates.iter().copied().collect();
        RecordBatch::try_new(
            metric_summaries_schema(),
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(StringArray::from(variant_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Float64Array::from(values.to_vec())),
                Arc::new(cov_arr),
            ],
        )
        .unwrap()
    }

    fn make_interleaving_data(
        exp_ids: &[&str],
        user_ids: &[&str],
        algo_scores: &[Vec<(&str, f64)>],
        winners: &[Option<&str>],
        engagements: &[i64],
    ) -> RecordBatch {
        let mut map_builder = MapBuilder::new(None, StringBuilder::new(), Float64Builder::new());
        for row_scores in algo_scores {
            for &(k, v) in row_scores {
                map_builder.keys().append_value(k);
                map_builder.values().append_value(v);
            }
            map_builder.append(true).unwrap();
        }
        let map_arr = map_builder.finish();
        let winner_arr: StringArray = winners.iter().copied().collect();
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("algorithm_scores", map_arr.data_type().clone(), false),
            Field::new("winning_algorithm_id", DataType::Utf8, true),
            Field::new("total_engagements", DataType::Int64, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(map_arr),
                Arc::new(winner_arr),
                Arc::new(Int64Array::from(engagements.to_vec())),
            ],
        )
        .unwrap()
    }

    fn make_daily_effects_data(
        exp_ids: &[&str],
        metric_ids: &[&str],
        dates: &[i32],
        effects: &[f64],
        sizes: &[i64],
    ) -> RecordBatch {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("effect_date", DataType::Date32, false),
            Field::new("absolute_effect", DataType::Float64, false),
            Field::new("sample_size", DataType::Int64, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Date32Array::from(dates.to_vec())),
                Arc::new(Float64Array::from(effects.to_vec())),
                Arc::new(Int64Array::from(sizes.to_vec())),
            ],
        )
        .unwrap()
    }

    // -----------------------------------------------------------------------
    // Proto converter tests (kept from original)
    // -----------------------------------------------------------------------

    fn sample_result_no_spillover() -> interference::InterferenceAnalysisResult {
        interference::InterferenceAnalysisResult {
            interference_detected: false,
            jensen_shannon_divergence: 0.01,
            jaccard_similarity_top_100: 0.95,
            treatment_gini_coefficient: 0.3,
            control_gini_coefficient: 0.32,
            treatment_catalog_coverage: 0.8,
            control_catalog_coverage: 0.75,
            spillover_titles: vec![],
        }
    }

    fn sample_result_with_spillover() -> interference::InterferenceAnalysisResult {
        interference::InterferenceAnalysisResult {
            interference_detected: true,
            jensen_shannon_divergence: 0.12,
            jaccard_similarity_top_100: 0.65,
            treatment_gini_coefficient: 0.45,
            control_gini_coefficient: 0.30,
            treatment_catalog_coverage: 0.70,
            control_catalog_coverage: 0.85,
            spillover_titles: vec![
                interference::TitleSpillover {
                    content_id: "movie-42".into(),
                    treatment_watch_rate: 0.15,
                    control_watch_rate: 0.05,
                    p_value: 0.001,
                },
                interference::TitleSpillover {
                    content_id: "movie-99".into(),
                    treatment_watch_rate: 0.08,
                    control_watch_rate: 0.02,
                    p_value: 0.01,
                },
            ],
        }
    }

    #[test]
    fn test_to_proto_interference_no_spillover() {
        let result = sample_result_no_spillover();
        let proto = to_proto_interference_result("exp-1", &result);
        assert_eq!(proto.experiment_id, "exp-1");
        assert!(!proto.interference_detected);
        assert!(proto.spillover_titles.is_empty());
        assert!(proto.computed_at.is_some());
    }

    #[test]
    fn test_to_proto_interference_with_spillover() {
        let result = sample_result_with_spillover();
        let proto = to_proto_interference_result("exp-2", &result);
        assert!(proto.interference_detected);
        assert_eq!(proto.spillover_titles.len(), 2);
        assert_eq!(proto.spillover_titles[0].content_id, "movie-42");
        assert_eq!(proto.spillover_titles[1].content_id, "movie-99");
    }

    #[test]
    fn test_to_proto_all_fields_mapped() {
        let result = sample_result_with_spillover();
        let proto = to_proto_interference_result("exp-3", &result);
        assert_eq!(proto.jensen_shannon_divergence, 0.12);
        assert_eq!(proto.jaccard_similarity_top_100, 0.65);
        assert_eq!(proto.treatment_gini_coefficient, 0.45);
        assert_eq!(proto.control_gini_coefficient, 0.30);
        assert_eq!(proto.treatment_catalog_coverage, 0.70);
        assert_eq!(proto.control_catalog_coverage, 0.85);
        let spill = &proto.spillover_titles[0];
        assert_eq!(spill.treatment_watch_rate, 0.15);
        assert_eq!(spill.control_watch_rate, 0.05);
        assert_eq!(spill.p_value, 0.001);
    }

    // -----------------------------------------------------------------------
    // RunAnalysis / GetAnalysisResult tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_analysis_basic() {
        let tmp = TempDir::new().unwrap();
        // 5 control users + 5 treatment users, metric "ctr", clear effect
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert_eq!(result.metric_results.len(), 1);

        let mr = &result.metric_results[0];
        assert_eq!(mr.metric_id, "ctr");
        assert_eq!(mr.variant_id, "treatment");
        assert!((mr.control_mean - 3.0).abs() < 1e-10);
        assert!((mr.treatment_mean - 13.0).abs() < 1e-10);
        assert!((mr.absolute_effect - 10.0).abs() < 1e-10);
        assert!(mr.is_significant); // large effect
        assert!(mr.p_value < 0.05);
        assert!(result.srm_result.is_some());
    }

    #[tokio::test]
    async fn test_run_analysis_with_cuped() {
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        // Highly correlated covariate and outcome
        let values: Vec<f64> = (0..10)
            .map(|i| (i as f64) * 2.0 + if i >= 5 { 3.0 } else { 1.0 })
            .collect();
        let covariates: Vec<Option<f64>> = (0..10).map(|i| Some(i as f64)).collect();

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let mr = &resp.into_inner().metric_results[0];
        // CUPED should be populated since all covariates are present
        assert!(
            mr.variance_reduction_pct > 0.0,
            "CUPED should reduce variance, got {}",
            mr.variance_reduction_pct
        );
    }

    #[tokio::test]
    async fn test_run_analysis_srm() {
        let tmp = TempDir::new().unwrap();
        // 50/50 split → no SRM
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 1.1, 2.1, 3.1, 4.1, 5.1];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let srm = resp.into_inner().srm_result.unwrap();
        assert!(!srm.is_mismatch, "50/50 split should not trigger SRM");
        assert_eq!(srm.observed_counts.len(), 2);
        assert_eq!(srm.expected_counts.len(), 2);
    }

    #[tokio::test]
    async fn test_run_analysis_empty_id() {
        let handler = test_handler("/tmp/nonexistent");
        let err = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "".into(),
                ..Default::default()
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_run_analysis_not_found() {
        let tmp = TempDir::new().unwrap();
        let batch =
            make_analysis_data(&["exp-1"], &["u1"], &["control"], &["ctr"], &[1.0], &[None]);
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let err = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-999".into(),
                ..Default::default()
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_get_analysis_result_delegates() {
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .get_analysis_result(Request::new(GetAnalysisResultRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert_eq!(result.metric_results.len(), 1);
        assert!(result.srm_result.is_some());
    }

    // -----------------------------------------------------------------------
    // GetInterleavingAnalysis tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_interleaving_basic() {
        let tmp = TempDir::new().unwrap();
        // 10 scores: 7 wins for algo_a, 3 for algo_b → sign test should detect
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = (0..n)
            .map(|i| match i {
                0 => "u0",
                1 => "u1",
                2 => "u2",
                3 => "u3",
                4 => "u4",
                5 => "u5",
                6 => "u6",
                7 => "u7",
                8 => "u8",
                _ => "u9",
            })
            .collect();
        let algo_scores: Vec<Vec<(&str, f64)>> = (0..n)
            .map(|i| {
                if i < 7 {
                    vec![("algo_a", 3.0), ("algo_b", 1.0)]
                } else {
                    vec![("algo_a", 1.0), ("algo_b", 3.0)]
                }
            })
            .collect();
        let winners: Vec<Option<&str>> = (0..n)
            .map(|i| {
                if i < 7 {
                    Some("algo_a")
                } else {
                    Some("algo_b")
                }
            })
            .collect();
        let engagements: Vec<i64> = vec![4; n];

        let batch =
            make_interleaving_data(&exp_ids, &user_ids, &algo_scores, &winners, &engagements);
        write_table(tmp.path(), "interleaving_scores", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert!(result.algorithm_win_rates.contains_key("algo_a"));
        assert!(result.algorithm_win_rates.contains_key("algo_b"));
        assert!(result.algorithm_win_rates["algo_a"] > result.algorithm_win_rates["algo_b"]);
        assert!(!result.algorithm_strengths.is_empty());
        assert!(!result.position_analyses.is_empty());
        assert!(result.computed_at.is_some());
    }

    #[tokio::test]
    async fn test_interleaving_empty_id() {
        let handler = test_handler("/tmp/nonexistent");
        let err = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_interleaving_not_found() {
        let tmp = TempDir::new().unwrap();
        let batch = make_interleaving_data(
            &["exp-1"],
            &["u1"],
            &[vec![("algo_a", 3.0), ("algo_b", 1.0)]],
            &[Some("algo_a")],
            &[4],
        );
        write_table(tmp.path(), "interleaving_scores", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let err = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "exp-999".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    // -----------------------------------------------------------------------
    // GetNoveltyAnalysis tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_novelty_basic() {
        let tmp = TempDir::new().unwrap();
        // 15 days of decaying effect: s=5, a=3, d=4
        let n = 15;
        let base_date = 19700i32;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let dates: Vec<i32> = (0..n as i32).map(|i| base_date + i).collect();
        let effects: Vec<f64> = (0..n)
            .map(|i| 5.0 + 3.0 * (-(i as f64) / 4.0).exp())
            .collect();
        let sizes: Vec<i64> = vec![1000; n];

        let batch = make_daily_effects_data(&exp_ids, &metric_ids, &dates, &effects, &sizes);
        write_table(tmp.path(), "daily_treatment_effects", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert_eq!(result.metric_id, "ctr");
        assert!(result.novelty_detected);
        assert!(result.decay_constant_days > 0.0);
        assert!(result.computed_at.is_some());
    }

    #[tokio::test]
    async fn test_novelty_empty_id() {
        let handler = test_handler("/tmp/nonexistent");
        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_novelty_not_found() {
        let tmp = TempDir::new().unwrap();
        let batch = make_daily_effects_data(&["exp-1"], &["ctr"], &[19700], &[5.0], &[1000]);
        write_table(tmp.path(), "daily_treatment_effects", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-999".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_novelty_insufficient_days() {
        let tmp = TempDir::new().unwrap();
        // Only 5 days → stats crate returns error (needs ≥ 7)
        let n = 5;
        let base_date = 19700i32;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let dates: Vec<i32> = (0..n as i32).map(|i| base_date + i).collect();
        let effects: Vec<f64> = vec![5.0; n];
        let sizes: Vec<i64> = vec![1000; n];

        let batch = make_daily_effects_data(&exp_ids, &metric_ids, &dates, &effects, &sizes);
        write_table(tmp.path(), "daily_treatment_effects", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }

    // -----------------------------------------------------------------------
    // GetInterferenceAnalysis tests (kept from original)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_interference_empty_experiment_id() {
        let handler = test_handler("/tmp/nonexistent");
        let err = handler
            .get_interference_analysis(Request::new(GetInterferenceAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    // -----------------------------------------------------------------------
    // CATE segment analysis tests
    // -----------------------------------------------------------------------

    fn metric_summaries_schema_with_segment() -> Arc<ArrowSchema> {
        Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("variant_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("metric_value", DataType::Float64, false),
            Field::new("cuped_covariate", DataType::Float64, true),
            Field::new("lifecycle_segment", DataType::Utf8, true),
        ]))
    }

    fn make_segmented_analysis_data(
        exp_ids: &[&str],
        user_ids: &[&str],
        variant_ids: &[&str],
        metric_ids: &[&str],
        values: &[f64],
        covariates: &[Option<f64>],
        segments: &[Option<&str>],
    ) -> RecordBatch {
        let cov_arr: Float64Array = covariates.iter().copied().collect();
        let seg_arr: StringArray = segments.iter().copied().collect();
        RecordBatch::try_new(
            metric_summaries_schema_with_segment(),
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(StringArray::from(variant_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Float64Array::from(values.to_vec())),
                Arc::new(cov_arr),
                Arc::new(seg_arr),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_run_analysis_with_segments_heterogeneous() {
        let tmp = TempDir::new().unwrap();
        // Two segments with different treatment effects:
        // TRIAL: control ~3, treatment ~4 (effect ~1)
        // ESTABLISHED: control ~3, treatment ~13 (effect ~10)
        let n = 20;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = (0..n)
            .map(|i| match i {
                0 => "u00",
                1 => "u01",
                2 => "u02",
                3 => "u03",
                4 => "u04",
                5 => "u05",
                6 => "u06",
                7 => "u07",
                8 => "u08",
                9 => "u09",
                10 => "u10",
                11 => "u11",
                12 => "u12",
                13 => "u13",
                14 => "u14",
                15 => "u15",
                16 => "u16",
                17 => "u17",
                18 => "u18",
                _ => "u19",
            })
            .collect();
        let variant_ids: Vec<&str> = vec![
            // TRIAL: 5 control + 5 treatment
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            // ESTABLISHED: 5 control + 5 treatment
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values: Vec<f64> = vec![
            // TRIAL control: ~3
            1.0, 2.0, 3.0, 4.0, 5.0, // TRIAL treatment: ~4 (effect = 1)
            2.0, 3.0, 4.0, 5.0, 6.0, // ESTABLISHED control: ~3
            1.0, 2.0, 3.0, 4.0, 5.0, // ESTABLISHED treatment: ~13 (effect = 10)
            11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let covariates: Vec<Option<f64>> = vec![None; n];
        let segments: Vec<Option<&str>> = vec![
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
        ];

        let batch = make_segmented_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
            &segments,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.metric_results.len(), 1);

        let mr = &result.metric_results[0];
        // Should have 2 segment results (TRIAL + ESTABLISHED)
        assert_eq!(
            mr.segment_results.len(),
            2,
            "expected 2 segment results, got {}",
            mr.segment_results.len()
        );

        // Verify segments are ESTABLISHED (3) and TRIAL (1)
        let seg_enums: Vec<i32> = mr.segment_results.iter().map(|s| s.segment).collect();
        assert!(seg_enums.contains(&3), "should contain ESTABLISHED (3)");
        assert!(seg_enums.contains(&1), "should contain TRIAL (1)");

        // ESTABLISHED should have a much larger effect than TRIAL
        let established = mr.segment_results.iter().find(|s| s.segment == 3).unwrap();
        let trial = mr.segment_results.iter().find(|s| s.segment == 1).unwrap();
        assert!(
            established.effect > trial.effect + 5.0,
            "ESTABLISHED effect ({}) should be much larger than TRIAL effect ({})",
            established.effect,
            trial.effect
        );

        // Cochran Q should detect heterogeneity (very different effects)
        assert!(
            result.cochran_q_p_value > 0.0,
            "cochran_q_p_value should be populated"
        );
        assert!(
            result.cochran_q_p_value < 0.05,
            "cochran_q_p_value {} should be < 0.05 for heterogeneous effects",
            result.cochran_q_p_value
        );
    }

    #[tokio::test]
    async fn test_run_analysis_without_segments() {
        // Data without lifecycle_segment column → segment_results should be empty
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        let mr = &result.metric_results[0];
        assert!(
            mr.segment_results.is_empty(),
            "segment_results should be empty without lifecycle_segment column"
        );
        assert!(
            (result.cochran_q_p_value - 0.0).abs() < 1e-10,
            "cochran_q_p_value should be 0.0 without segments"
        );
    }

    #[tokio::test]
    async fn test_run_analysis_homogeneous_segments() {
        let tmp = TempDir::new().unwrap();
        // Two segments with identical treatment effects → Q should be small
        let n = 20;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = (0..n)
            .map(|i| match i {
                0 => "u00",
                1 => "u01",
                2 => "u02",
                3 => "u03",
                4 => "u04",
                5 => "u05",
                6 => "u06",
                7 => "u07",
                8 => "u08",
                9 => "u09",
                10 => "u10",
                11 => "u11",
                12 => "u12",
                13 => "u13",
                14 => "u14",
                15 => "u15",
                16 => "u16",
                17 => "u17",
                18 => "u18",
                _ => "u19",
            })
            .collect();
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        // Both segments: same effect = 1.0
        let values: Vec<f64> = vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 2.0, 3.0, 4.0, 5.0, 6.0, 10.0, 11.0, 12.0, 13.0, 14.0, 11.0,
            12.0, 13.0, 14.0, 15.0,
        ];
        let covariates: Vec<Option<f64>> = vec![None; n];
        let segments: Vec<Option<&str>> = vec![
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("TRIAL"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
            Some("ESTABLISHED"),
        ];

        let batch = make_segmented_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
            &segments,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        let mr = &result.metric_results[0];
        assert_eq!(mr.segment_results.len(), 2);

        // Effects should be approximately equal (~1.0)
        for seg in &mr.segment_results {
            assert!(
                (seg.effect - 1.0).abs() < 1e-10,
                "segment {} effect {} should be ~1.0",
                seg.segment,
                seg.effect
            );
        }

        // Cochran Q should NOT detect heterogeneity
        assert!(
            result.cochran_q_p_value > 0.05,
            "cochran_q_p_value {} should be > 0.05 for homogeneous effects",
            result.cochran_q_p_value
        );
    }

    // -----------------------------------------------------------------------
    // IPW-adjusted analysis tests
    // -----------------------------------------------------------------------

    fn metric_summaries_schema_with_ipw() -> Arc<ArrowSchema> {
        Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("variant_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("metric_value", DataType::Float64, false),
            Field::new("cuped_covariate", DataType::Float64, true),
            Field::new("assignment_probability", DataType::Float64, true),
        ]))
    }

    fn make_ipw_analysis_data(
        exp_ids: &[&str],
        user_ids: &[&str],
        variant_ids: &[&str],
        metric_ids: &[&str],
        values: &[f64],
        covariates: &[Option<f64>],
        assignment_probs: &[Option<f64>],
    ) -> RecordBatch {
        let cov_arr: Float64Array = covariates.iter().copied().collect();
        let prob_arr: Float64Array = assignment_probs.iter().copied().collect();
        RecordBatch::try_new(
            metric_summaries_schema_with_ipw(),
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(StringArray::from(variant_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Float64Array::from(values.to_vec())),
                Arc::new(cov_arr),
                Arc::new(prob_arr),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_run_analysis_with_ipw() {
        let tmp = TempDir::new().unwrap();
        // 5 control + 5 treatment, with varying assignment probabilities (bandit-style)
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> =
            vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];
        // Bandit-style: control gets 70%, treatment gets 30%
        let probs: Vec<Option<f64>> = vec![
            Some(0.7),
            Some(0.7),
            Some(0.7),
            Some(0.7),
            Some(0.7),
            Some(0.3),
            Some(0.3),
            Some(0.3),
            Some(0.3),
            Some(0.3),
        ];

        let batch = make_ipw_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
            &probs,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.metric_results.len(), 1);

        let mr = &result.metric_results[0];
        // IPW result should be populated
        assert!(
            mr.ipw_result.is_some(),
            "ipw_result should be populated when assignment_probability is available"
        );
        let ipw = mr.ipw_result.as_ref().unwrap();
        // IPW effect should be positive (treatment > control)
        assert!(
            ipw.effect > 0.0,
            "IPW effect {} should be positive",
            ipw.effect
        );
        // SE should be positive
        assert!(ipw.se > 0.0, "IPW SE {} should be positive", ipw.se);
        // CI should bracket the effect
        assert!(ipw.ci_lower < ipw.effect);
        assert!(ipw.ci_upper > ipw.effect);
        // p-value should be significant for this large effect
        assert!(
            ipw.p_value < 0.05,
            "IPW p_value {} should be < 0.05",
            ipw.p_value
        );
        // ESS should be positive and less than N
        assert!(ipw.effective_sample_size > 0.0);
        assert!(ipw.effective_sample_size <= n as f64);
    }

    #[tokio::test]
    async fn test_run_analysis_without_ipw_column() {
        // Standard data without assignment_probability → ipw_result should be None
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> =
            vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control",
            "control",
            "control",
            "control",
            "control",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
            "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        let mr = &result.metric_results[0];
        // Without assignment_probability column, IPW result should be None
        assert!(
            mr.ipw_result.is_none(),
            "ipw_result should be None when assignment_probability is not available"
        );
    }

    // -----------------------------------------------------------------------
    // ADR-015: AVLM produces narrower CIs than mSPRT on golden-file data
    // -----------------------------------------------------------------------

    /// Integration test: AVLM with a strong covariate produces a narrower confidence
    /// sequence than AVLM without covariate (mSPRT fallback), matching ADR-015 claim.
    ///
    /// Golden-file data: control and treatment both have outcomes linearly correlated
    /// with the covariate (ρ ≈ 0.99). Regression adjustment removes most variance,
    /// shrinking the half-width of the confidence sequence.
    #[tokio::test]
    async fn test_run_analysis_avlm_narrower_ci_than_msprt() {
        let tmp = TempDir::new().unwrap();
        let n = 30;
        let exp_ids: Vec<&str> = vec!["exp-avlm"; n];
        let user_ids: Vec<String> = (0..n).map(|i| format!("u{i}")).collect();
        let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
        let metric_ids: Vec<&str> = vec!["metric"; n];

        // 15 control + 15 treatment. Outcome y = 2*x + noise + treatment_effect.
        // Strong covariate (x = 0..14 for control, 0..14 for treatment) → ρ ≈ 0.99.
        // True effect = 3.0. With 15 obs/arm this should be detectable by both, but
        // AVLM half-width will be markedly narrower due to variance reduction.
        let mut variant_ids: Vec<&str> = Vec::with_capacity(n);
        let mut values: Vec<f64> = Vec::with_capacity(n);
        let mut covariates: Vec<Option<f64>> = Vec::with_capacity(n);

        for i in 0..15usize {
            variant_ids.push("control");
            let x = i as f64;
            values.push(2.0 * x + 0.1); // near-deterministic
            covariates.push(Some(x));
        }
        for i in 0..15usize {
            variant_ids.push("treatment");
            let x = i as f64;
            values.push(2.0 * x + 3.0 + 0.1); // effect = 3.0
            covariates.push(Some(x));
        }

        let batch = make_analysis_data(
            &exp_ids,
            &user_id_refs,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());

        // Run 1: AVLM with covariate adjustment (cuped_covariate_metric_id set).
        let resp_avlm = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-avlm".into(),
                sequential_method: SEQUENTIAL_METHOD_AVLM,
                tau_sq: 0.5,
                cuped_covariate_metric_id: "pre_experiment_metric".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        // Run 2: AVLM without covariate (mSPRT fallback; cuped_covariate_metric_id empty).
        let resp_msprt = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-avlm".into(),
                sequential_method: SEQUENTIAL_METHOD_AVLM,
                tau_sq: 0.5,
                cuped_covariate_metric_id: "".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let mr_avlm = &resp_avlm.into_inner().metric_results[0];
        let mr_msprt = &resp_msprt.into_inner().metric_results[0];

        // Both should have sequential results.
        assert!(
            mr_avlm.sequential_result.is_some(),
            "AVLM should produce a sequential_result"
        );
        assert!(
            mr_msprt.sequential_result.is_some(),
            "mSPRT (no covariate) should produce a sequential_result"
        );

        // AVLM CI half-width should be strictly narrower than mSPRT.
        let avlm_half_width = mr_avlm.cuped_ci_upper - mr_avlm.cuped_ci_lower;
        let msprt_half_width = mr_msprt.cuped_ci_upper - mr_msprt.cuped_ci_lower;
        assert!(
            avlm_half_width < msprt_half_width,
            "AVLM half-width ({avlm_half_width:.4}) should be narrower than mSPRT ({msprt_half_width:.4})"
        );

        // AVLM variance reduction should be substantial (ρ ≈ 0.99 → ~98% reduction).
        assert!(
            mr_avlm.variance_reduction_pct > 50.0,
            "Expected >50% variance reduction from strong covariate, got {}",
            mr_avlm.variance_reduction_pct
        );
    }

    // -----------------------------------------------------------------------
    // ADR-020: adaptive_n wiring — zone classification returned in AnalysisResult
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_analysis_adaptive_n_zone_returned() {
        use experimentation_proto::experimentation::common::v1::AdaptiveSampleSizeConfig;

        let tmp = TempDir::new().unwrap();
        let n = 20;
        let exp_ids: Vec<&str> = vec!["exp-adaptive"; n];
        let user_ids: Vec<String> = (0..n).map(|i| format!("u{i}")).collect();
        let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
        let metric_ids: Vec<&str> = vec!["metric"; n];

        // 10 control + 10 treatment, moderate effect.
        let mut variant_ids: Vec<&str> = Vec::with_capacity(n);
        let mut values: Vec<f64> = Vec::with_capacity(n);
        let covariates: Vec<Option<f64>> = vec![None; n];

        for i in 0..10usize {
            variant_ids.push("control");
            values.push(1.0 + (i as f64) * 0.1);
        }
        for i in 0..10usize {
            variant_ids.push("treatment");
            values.push(1.5 + (i as f64) * 0.1);
        }

        let batch = make_analysis_data(
            &exp_ids,
            &user_id_refs,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-adaptive".into(),
                adaptive_sample_size_config: Some(AdaptiveSampleSizeConfig {
                    interim_fraction: 0.5,
                    promising_zone_lower: 0.30,
                    favorable_zone_lower: 0.90,
                    max_extension_factor: 2.0,
                }),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();

        // adaptive_n_result must be populated when config is provided.
        let adaptive = result
            .adaptive_n_result
            .expect("adaptive_n_result must be set when AdaptiveSampleSizeConfig provided");

        // Zone must be one of the valid classifications.
        assert!(
            ["favorable", "promising", "futile"].contains(&adaptive.zone.as_str()),
            "zone must be favorable, promising, or futile; got '{}'",
            adaptive.zone
        );

        // Conditional power must be in [0, 1].
        assert!(
            (0.0..=1.0).contains(&adaptive.conditional_power),
            "conditional_power must be in [0, 1], got {}",
            adaptive.conditional_power
        );

        // Blinded variance must be positive.
        assert!(
            adaptive.blinded_variance > 0.0,
            "blinded_variance must be positive, got {}",
            adaptive.blinded_variance
        );

        // For promising zone, recommended_n_per_arm must be positive.
        if adaptive.zone == "promising" {
            assert!(
                adaptive.recommended_n_per_arm > 0.0,
                "recommended_n_per_arm must be >0 in promising zone"
            );
        }
    }

    #[tokio::test]
    async fn test_run_analysis_no_adaptive_n_when_config_absent() {
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control", "control", "control", "control", "control",
            "treatment", "treatment", "treatment", "treatment", "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(
            &exp_ids,
            &user_ids,
            &variant_ids,
            &metric_ids,
            &values,
            &covariates,
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = test_handler(tmp.path().to_str().unwrap());
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert!(
            result.adaptive_n_result.is_none(),
            "adaptive_n_result must be None when no AdaptiveSampleSizeConfig is provided"
        );
    }
}
