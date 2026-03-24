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
    Experiment, ExperimentType, MetaExperimentConfig, QuasiExperimentConfig, SwitchbackConfig,
};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Validate type-specific STARTING preconditions.
///
/// Returns `Ok(())` if validation passes, or `Status::failed_precondition`
/// with a descriptive message on failure.
pub fn validate_starting(exp: &Experiment) -> Result<(), Status> {
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

fn validate_meta(exp: &Experiment) -> Result<(), Status> {
    let cfg = exp.meta_experiment_config.as_ref().ok_or_else(|| {
        Status::failed_precondition("META experiment requires meta_experiment_config")
    })?;

    validate_meta_config(cfg, &exp.variants.iter().map(|v| v.variant_id.as_str()).collect::<Vec<_>>())
}

fn validate_meta_config(
    cfg: &MetaExperimentConfig,
    variant_ids: &[&str],
) -> Result<(), Status> {
    use experimentation_proto::experimentation::common::v1::BanditAlgorithm;

    let algo = BanditAlgorithm::try_from(cfg.base_algorithm).unwrap_or(BanditAlgorithm::Unspecified);
    if algo == BanditAlgorithm::Unspecified {
        return Err(Status::failed_precondition(
            "META experiment requires a valid base_algorithm in meta_experiment_config",
        ));
    }

    if cfg.outcome_metric_ids.is_empty() {
        return Err(Status::failed_precondition(
            "META experiment requires at least one outcome_metric_id",
        ));
    }

    if cfg.variant_objectives.len() != variant_ids.len() {
        return Err(Status::failed_precondition(format!(
            "META experiment: variant_objectives count ({}) must equal variant count ({})",
            cfg.variant_objectives.len(),
            variant_ids.len()
        )));
    }

    for obj in &cfg.variant_objectives {
        // reward_weights must sum to 1.0 (within tolerance).
        let sum: f64 = obj.reward_weights.values().sum();
        if (sum - 1.0).abs() > 1e-6 {
            return Err(Status::failed_precondition(format!(
                "META variant {} reward_weights sum to {:.6} (must be 1.0)",
                obj.variant_id, sum
            )));
        }

        if obj.reward_weights.is_empty() {
            return Err(Status::failed_precondition(format!(
                "META variant {} has no reward_weights",
                obj.variant_id
            )));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// SWITCHBACK validator (ADR-022)
// ---------------------------------------------------------------------------

fn validate_switchback(exp: &Experiment) -> Result<(), Status> {
    let cfg = exp.switchback_config.as_ref().ok_or_else(|| {
        Status::failed_precondition("SWITCHBACK experiment requires switchback_config")
    })?;

    validate_switchback_config(cfg)
}

fn validate_switchback_config(cfg: &SwitchbackConfig) -> Result<(), Status> {
    if cfg.planned_cycles < 4 {
        return Err(Status::failed_precondition(format!(
            "SWITCHBACK requires planned_cycles >= 4 (got {})",
            cfg.planned_cycles
        )));
    }

    let block_secs = cfg
        .block_duration
        .as_ref()
        .map(|d| d.seconds)
        .unwrap_or(0);

    if block_secs < 3600 {
        return Err(Status::failed_precondition(format!(
            "SWITCHBACK requires block_duration >= 1 hour (got {} seconds)",
            block_secs
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// QUASI validator (ADR-023)
// ---------------------------------------------------------------------------

fn validate_quasi(exp: &Experiment) -> Result<(), Status> {
    let cfg = exp.quasi_experiment_config.as_ref().ok_or_else(|| {
        Status::failed_precondition("QUASI experiment requires quasi_experiment_config")
    })?;

    validate_quasi_config(cfg)
}

fn validate_quasi_config(cfg: &QuasiExperimentConfig) -> Result<(), Status> {
    if cfg.treated_unit_id.trim().is_empty() {
        return Err(Status::failed_precondition(
            "QUASI experiment requires a non-empty treated_unit_id",
        ));
    }

    if cfg.donor_unit_ids.len() < 2 {
        return Err(Status::failed_precondition(format!(
            "QUASI experiment requires at least 2 donor_unit_ids (got {})",
            cfg.donor_unit_ids.len()
        )));
    }

    if cfg.outcome_metric_id.trim().is_empty() {
        return Err(Status::failed_precondition(
            "QUASI experiment requires outcome_metric_id",
        ));
    }

    // Validate temporal ordering.
    let pre_start = cfg.pre_treatment_start.as_ref();
    let treatment_start = cfg.treatment_start.as_ref();

    match (pre_start, treatment_start) {
        (Some(pre), Some(treat)) => {
            let pre_ts = pre.seconds * 1_000_000_000 + pre.nanos as i64;
            let treat_ts = treat.seconds * 1_000_000_000 + treat.nanos as i64;
            if pre_ts >= treat_ts {
                return Err(Status::failed_precondition(
                    "QUASI experiment: pre_treatment_start must be before treatment_start",
                ));
            }
        }
        (None, _) => {
            return Err(Status::failed_precondition(
                "QUASI experiment requires pre_treatment_start",
            ));
        }
        (_, None) => {
            return Err(Status::failed_precondition(
                "QUASI experiment requires treatment_start",
            ));
        }
    }

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
}
