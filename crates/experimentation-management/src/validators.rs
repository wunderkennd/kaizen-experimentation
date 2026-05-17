//! STARTING-gate validators for Phase 5 experiment types.
//!
//! Each validator is called during the DRAFT→STARTING transition, before
//! bucket allocation. On failure the experiment reverts to DRAFT.
//!
//! Types validated here:
//!   - META (ADR-013): MetaExperimentConfig required; reward weights per variant sum to 1.0.
//!   - SWITCHBACK (ADR-022): planned_cycles >= 4; block_duration >= 1h.
//!   - QUASI (ADR-023): treated unit + >= 2 donors; pre_treatment_start < treatment_start.
//!
//! For other types (AB, MAB, etc.) validation is delegated to the existing
//! CreateExperiment logic (traffic fractions, control variant, etc.).

use tonic::Status;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, Experiment, ExperimentType,
    FilteredMeanConfig, MetaExperimentConfig, MetricDefinition, MetricType,
    QuasiExperimentConfig, SwitchbackConfig, WindowedCountConfig,
};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Validate type-specific STARTING preconditions.
///
/// Returns `Ok(())` if validation passes, or `Box<Status::failed_precondition>`
/// with a descriptive message on failure.
pub fn validate_starting(exp: &Experiment) -> Result<(), Box<Status>> {
    let exp_type = ExperimentType::try_from(exp.r#type).unwrap_or(ExperimentType::Unspecified);

    match exp_type {
        ExperimentType::Meta => validate_meta(exp),
        ExperimentType::Switchback => validate_switchback(exp),
        ExperimentType::Quasi => validate_quasi(exp),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// META validator (ADR-013)
// ---------------------------------------------------------------------------

fn validate_meta(exp: &Experiment) -> Result<(), Box<Status>> {
    let cfg = exp.meta_experiment_config.as_ref().ok_or_else(|| {
        Box::new(Status::failed_precondition(
            "META experiment requires meta_experiment_config",
        ))
    })?;

    validate_meta_config(cfg, &exp.variants.iter().map(|v| v.variant_id.as_str()).collect::<Vec<_>>())
}

fn validate_meta_config(
    cfg: &MetaExperimentConfig,
    variant_ids: &[&str],
) -> Result<(), Box<Status>> {
    use experimentation_proto::experimentation::common::v1::BanditAlgorithm;

    let algo = BanditAlgorithm::try_from(cfg.base_algorithm).unwrap_or(BanditAlgorithm::Unspecified);
    if algo == BanditAlgorithm::Unspecified {
        return Err(Box::new(Status::failed_precondition(
            "META experiment requires a valid base_algorithm in meta_experiment_config",
        )));
    }

    if cfg.outcome_metric_ids.is_empty() {
        return Err(Box::new(Status::failed_precondition(
            "META experiment requires at least one outcome_metric_id",
        )));
    }

    if cfg.variant_objectives.len() != variant_ids.len() {
        return Err(Box::new(Status::failed_precondition(format!(
            "META experiment: variant_objectives count ({}) must equal variant count ({})",
            cfg.variant_objectives.len(),
            variant_ids.len()
        ))));
    }

    for obj in &cfg.variant_objectives {
        if obj.reward_weights.is_empty() {
            return Err(Box::new(Status::failed_precondition(format!(
                "META variant {} has no reward_weights",
                obj.variant_id
            ))));
        }

        // Reject NaN/Infinity reward weights (IEEE 754 NaN comparisons silently pass).
        for (key, &w) in &obj.reward_weights {
            if !w.is_finite() {
                return Err(Box::new(Status::failed_precondition(format!(
                    "META variant {} reward_weight '{}' is non-finite",
                    obj.variant_id, key
                ))));
            }
        }

        // reward_weights must sum to 1.0 (within tolerance).
        let sum: f64 = obj.reward_weights.values().sum();
        if (sum - 1.0).abs() > 1e-6 {
            return Err(Box::new(Status::failed_precondition(format!(
                "META variant {} reward_weights sum to {:.6} (must be 1.0)",
                obj.variant_id, sum
            ))));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// SWITCHBACK validator (ADR-022)
// ---------------------------------------------------------------------------

fn validate_switchback(exp: &Experiment) -> Result<(), Box<Status>> {
    let cfg = exp.switchback_config.as_ref().ok_or_else(|| {
        Box::new(Status::failed_precondition(
            "SWITCHBACK experiment requires switchback_config",
        ))
    })?;

    validate_switchback_config(cfg)
}

fn validate_switchback_config(cfg: &SwitchbackConfig) -> Result<(), Box<Status>> {
    if cfg.planned_cycles < 4 {
        return Err(Box::new(Status::failed_precondition(format!(
            "SWITCHBACK requires planned_cycles >= 4 (got {})",
            cfg.planned_cycles
        ))));
    }

    let block_secs = cfg
        .block_duration
        .as_ref()
        .map(|d| d.seconds)
        .unwrap_or(0);

    if block_secs < 3600 {
        return Err(Box::new(Status::failed_precondition(format!(
            "SWITCHBACK requires block_duration >= 1 hour (got {} seconds)",
            block_secs
        ))));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// QUASI validator (ADR-023)
// ---------------------------------------------------------------------------

fn validate_quasi(exp: &Experiment) -> Result<(), Box<Status>> {
    let cfg = exp.quasi_experiment_config.as_ref().ok_or_else(|| {
        Box::new(Status::failed_precondition(
            "QUASI experiment requires quasi_experiment_config",
        ))
    })?;

    validate_quasi_config(cfg)
}

fn validate_quasi_config(cfg: &QuasiExperimentConfig) -> Result<(), Box<Status>> {
    if cfg.treated_unit_id.trim().is_empty() {
        return Err(Box::new(Status::failed_precondition(
            "QUASI experiment requires a non-empty treated_unit_id",
        )));
    }

    if cfg.donor_unit_ids.len() < 2 {
        return Err(Box::new(Status::failed_precondition(format!(
            "QUASI experiment requires at least 2 donor_unit_ids (got {})",
            cfg.donor_unit_ids.len()
        ))));
    }

    if cfg.outcome_metric_id.trim().is_empty() {
        return Err(Box::new(Status::failed_precondition(
            "QUASI experiment requires outcome_metric_id",
        )));
    }

    // Validate temporal ordering.
    let pre_start = cfg.pre_treatment_start.as_ref();
    let treatment_start = cfg.treatment_start.as_ref();

    match (pre_start, treatment_start) {
        (Some(pre), Some(treat)) => {
            let pre_ts = pre.seconds * 1_000_000_000 + pre.nanos as i64;
            let treat_ts = treat.seconds * 1_000_000_000 + treat.nanos as i64;
            if pre_ts >= treat_ts {
                return Err(Box::new(Status::failed_precondition(
                    "QUASI experiment: pre_treatment_start must be before treatment_start",
                )));
            }
        }
        (None, _) => {
            return Err(Box::new(Status::failed_precondition(
                "QUASI experiment requires pre_treatment_start",
            )));
        }
        (_, None) => {
            return Err(Box::new(Status::failed_precondition(
                "QUASI experiment requires treatment_start",
            )));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// MetricDefinition validator skeleton (ADR-026 Phase 1)
// ---------------------------------------------------------------------------
//
// `validate_metric_definition` is the gRPC-side entry point invoked by
// `create_metric_definition`. Phase 1 (#433) ships this skeleton:
//
//   - `validate_metric_common_fields` enforces minimum sanity checks
//     (non-empty `metric_id` + non-empty `name`).
//   - The 3 per-type validators (`validate_filtered_mean`,
//     `validate_composite`, `validate_windowed_count`) are intentionally
//     no-ops at this stage — Phase B fills them in:
//       * B1 — COMPOSITE operand arity, weight, DFS cycle detection. B1 will
//         refactor this entry point to async and add a `&ManagementStore`
//         parameter (operand existence + cycle walk both require store reads).
//       * B2 — WINDOWED_COUNT event_type regex + window_hours range +
//         optional filter_sql allowlist.
//       * B3 — FILTERED_MEAN value_column regex + filter_sql allowlist
//         parser (positive operator/identifier/literal allowlist).
//
// Stakeholder / aggregation_level are NOT enforced here: migration 007
// defaults both to empty-string when unset, and the existing flat-six
// metric flow tolerates that. Tightening those checks is deferred until
// Phase 2 (#436) when the M6 UI fully owns those inputs.

#[allow(clippy::result_large_err)]
pub fn validate_metric_definition(m: &MetricDefinition) -> Result<(), Box<Status>> {
    validate_metric_common_fields(m)?;
    match m.r#type() {
        MetricType::FilteredMean => validate_filtered_mean(filtered_mean_cfg(m)),
        // COMPOSITE rules (arity, weight, cycle detection) land in B1, which
        // will also refactor this dispatch to async + take a `&ManagementStore`.
        MetricType::Composite => Ok(()),
        MetricType::WindowedCount => validate_windowed_count(windowed_count_cfg(m)),
        // Legacy 6 types (MEAN, PROPORTION, RATIO, COUNT, PERCENTILE, CUSTOM)
        // and UNSPECIFIED fall through — existing flat-field validation, if
        // any, lives elsewhere or has historically been absent for Phase 1.
        _ => Ok(()),
    }
}

#[allow(clippy::result_large_err)]
fn validate_metric_common_fields(m: &MetricDefinition) -> Result<(), Box<Status>> {
    if m.metric_id.trim().is_empty() {
        return Err(Box::new(Status::invalid_argument(
            "metric_id is required",
        )));
    }
    if m.name.trim().is_empty() {
        return Err(Box::new(Status::invalid_argument(
            "metric.name is required",
        )));
    }
    Ok(())
}

// Helpers: extract the oneof arm payload for the per-type validators.
// prost generates `pub type_config: Option<TypeConfig>` (no `pub fn
// filtered_mean()` accessor), so we destructure manually.
fn filtered_mean_cfg(m: &MetricDefinition) -> Option<&FilteredMeanConfig> {
    match m.type_config.as_ref()? {
        MetricTypeConfig::FilteredMean(cfg) => Some(cfg),
        _ => None,
    }
}

fn windowed_count_cfg(m: &MetricDefinition) -> Option<&WindowedCountConfig> {
    match m.type_config.as_ref()? {
        MetricTypeConfig::WindowedCount(cfg) => Some(cfg),
        _ => None,
    }
}

#[allow(clippy::result_large_err)]
fn validate_filtered_mean(_cfg: Option<&FilteredMeanConfig>) -> Result<(), Box<Status>> {
    // Filled in by B3 (value_column regex + filter_sql allowlist parser).
    Ok(())
}

#[allow(clippy::result_large_err)]
fn validate_windowed_count(_cfg: Option<&WindowedCountConfig>) -> Result<(), Box<Status>> {
    // Filled in by B2 (event_type regex + window_hours range + filter_sql).
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use experimentation_proto::experimentation::common::v1::{
        BanditAlgorithm, MetaVariantObjective, SyntheticControlMethod,
    };
    use prost_types::Duration;

    fn make_meta_config(_cycles: i32, weights_per_variant: Vec<f64>) -> MetaExperimentConfig {
        let variant_objectives = weights_per_variant
            .iter()
            .enumerate()
            .map(|(i, &w)| MetaVariantObjective {
                variant_id: format!("v{i}"),
                reward_weights: [("metric_a".to_string(), w)].into(),
            })
            .collect();
        MetaExperimentConfig {
            base_algorithm: BanditAlgorithm::ThompsonSampling as i32,
            variant_objectives,
            outcome_metric_ids: vec!["business_outcome".to_string()],
        }
    }

    #[test]
    fn meta_valid() {
        let cfg = make_meta_config(2, vec![1.0, 1.0]);
        let variant_ids = vec!["v0", "v1"];
        assert!(validate_meta_config(&cfg, &variant_ids).is_ok());
    }

    #[test]
    fn meta_weights_must_sum_to_one() {
        let cfg = make_meta_config(2, vec![0.5, 0.5]); // 0.5 != 1.0 per variant
        let variant_ids = vec!["v0", "v1"];
        // v0 has 0.5, v1 has 0.5 — each variant's weights must sum to 1.0
        let err = validate_meta_config(&cfg, &variant_ids).unwrap_err();
        assert!(err.message().contains("reward_weights sum to"));
    }

    #[test]
    fn meta_count_mismatch() {
        let cfg = make_meta_config(2, vec![1.0]);
        let variant_ids = vec!["v0", "v1"]; // 2 variants but 1 objective
        let err = validate_meta_config(&cfg, &variant_ids).unwrap_err();
        assert!(err.message().contains("variant_objectives count"));
    }

    #[test]
    fn switchback_valid() {
        let cfg = SwitchbackConfig {
            planned_cycles: 6,
            block_duration: Some(Duration { seconds: 7200, nanos: 0 }),
            cluster_attribute: "market_id".into(),
            washout_period: None,
        };
        assert!(validate_switchback_config(&cfg).is_ok());
    }

    #[test]
    fn switchback_too_few_cycles() {
        let cfg = SwitchbackConfig {
            planned_cycles: 3,
            block_duration: Some(Duration { seconds: 3600, nanos: 0 }),
            cluster_attribute: String::new(),
            washout_period: None,
        };
        let err = validate_switchback_config(&cfg).unwrap_err();
        assert!(err.message().contains("planned_cycles >= 4"));
    }

    #[test]
    fn switchback_block_too_short() {
        let cfg = SwitchbackConfig {
            planned_cycles: 4,
            block_duration: Some(Duration { seconds: 1800, nanos: 0 }), // 30 min
            cluster_attribute: String::new(),
            washout_period: None,
        };
        let err = validate_switchback_config(&cfg).unwrap_err();
        assert!(err.message().contains("block_duration >= 1 hour"));
    }

    #[test]
    fn quasi_valid() {
        let cfg = QuasiExperimentConfig {
            treated_unit_id: "market_us".into(),
            donor_unit_ids: vec!["market_ca".into(), "market_uk".into()],
            pre_treatment_start: Some(prost_types::Timestamp { seconds: 1000, nanos: 0 }),
            treatment_start: Some(prost_types::Timestamp { seconds: 2000, nanos: 0 }),
            outcome_metric_id: "churn_rate".into(),
            method: SyntheticControlMethod::Augmented as i32,
        };
        assert!(validate_quasi_config(&cfg).is_ok());
    }

    #[test]
    fn quasi_temporal_violation() {
        let cfg = QuasiExperimentConfig {
            treated_unit_id: "market_us".into(),
            donor_unit_ids: vec!["a".into(), "b".into()],
            pre_treatment_start: Some(prost_types::Timestamp { seconds: 2000, nanos: 0 }),
            treatment_start: Some(prost_types::Timestamp { seconds: 1000, nanos: 0 }),
            outcome_metric_id: "metric".into(),
            method: SyntheticControlMethod::Classic as i32,
        };
        let err = validate_quasi_config(&cfg).unwrap_err();
        assert!(err.message().contains("pre_treatment_start must be before"));
    }

    #[test]
    fn quasi_too_few_donors() {
        let cfg = QuasiExperimentConfig {
            treated_unit_id: "t".into(),
            donor_unit_ids: vec!["one".into()],
            pre_treatment_start: Some(prost_types::Timestamp { seconds: 1000, nanos: 0 }),
            treatment_start: Some(prost_types::Timestamp { seconds: 2000, nanos: 0 }),
            outcome_metric_id: "metric".into(),
            method: SyntheticControlMethod::Classic as i32,
        };
        let err = validate_quasi_config(&cfg).unwrap_err();
        assert!(err.message().contains("at least 2 donor_unit_ids"));
    }

    // -----------------------------------------------------------------------
    // MetricDefinition skeleton (ADR-026 Phase 1)
    // -----------------------------------------------------------------------

    use experimentation_proto::experimentation::common::v1::{
        CompositeConfig, FilteredMeanConfig, WindowedCountConfig,
    };

    fn make_metric(metric_id: &str, name: &str, t: MetricType) -> MetricDefinition {
        MetricDefinition {
            metric_id: metric_id.into(),
            name: name.into(),
            r#type: t as i32,
            ..Default::default()
        }
    }

    #[test]
    fn metric_common_fields_reject_empty_metric_id() {
        let m = make_metric("", "Watch time", MetricType::Mean);
        let err = validate_metric_definition(&m).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("metric_id"));
    }

    #[test]
    fn metric_common_fields_reject_empty_name() {
        let m = make_metric("watch_time", "", MetricType::Mean);
        let err = validate_metric_definition(&m).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("name"));
    }

    #[test]
    fn metric_common_fields_reject_whitespace_only() {
        let m = make_metric("   ", "ok", MetricType::Mean);
        let err = validate_metric_definition(&m).unwrap_err();
        assert!(err.message().contains("metric_id"));
    }

    #[test]
    fn metric_legacy_mean_passes_through() {
        let m = make_metric("watch_time", "Watch time", MetricType::Mean);
        assert!(validate_metric_definition(&m).is_ok());
    }

    #[test]
    fn metric_filtered_mean_skeleton_passes() {
        // B3 fills in the value_column + filter_sql rules.
        let mut m = make_metric("filtered_dur", "Filtered duration", MetricType::FilteredMean);
        m.type_config = Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql: "platform = 'mobile'".into(),
            value_column: "duration_ms".into(),
        }));
        assert!(validate_metric_definition(&m).is_ok());
    }

    #[test]
    fn metric_composite_skeleton_passes() {
        // B1 fills in arity, weight, and cycle detection.
        let mut m = make_metric("composite_a", "Composite A", MetricType::Composite);
        m.type_config = Some(MetricTypeConfig::Composite(CompositeConfig::default()));
        assert!(validate_metric_definition(&m).is_ok());
    }

    #[test]
    fn metric_windowed_count_skeleton_passes() {
        // B2 fills in event_type regex + window_hours range.
        let mut m = make_metric("first_signup", "First signup", MetricType::WindowedCount);
        m.type_config = Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
            event_type: "signup_completed".into(),
            filter_sql: String::new(),
            window_hours: 24,
        }));
        assert!(validate_metric_definition(&m).is_ok());
    }
}
