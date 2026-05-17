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

use std::sync::OnceLock;

use regex::Regex;
use tonic::Status;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, CompositeConfig, CompositeOperator,
    Experiment, ExperimentType, FilteredMeanConfig, MetaExperimentConfig, MetricDefinition,
    MetricType, QuasiExperimentConfig, SwitchbackConfig, WindowedCountConfig,
};

pub mod composite_cycle;

// ---------------------------------------------------------------------------
// Shared identifier regex (used by B2 WINDOWED_COUNT.event_type and, when B3
// lands, FILTERED_MEAN.value_column). Compiled once via OnceLock.
// ---------------------------------------------------------------------------
fn identifier_re() -> &'static Regex {
    static IDENT_RE: OnceLock<Regex> = OnceLock::new();
    IDENT_RE.get_or_init(|| {
        Regex::new(r"^[a-z_][a-z0-9_]*$").expect("identifier regex is a compile-time constant")
    })
}

use crate::store::{ManagementStore, StoreError};

// ---------------------------------------------------------------------------
// MetricLookup — the minimal surface the COMPOSITE validator needs.
//
// Both the PG-backed `ManagementStore` and the in-memory `MetricStore`
// (`contract_test_support`) implement this so `validate_metric_definition`
// can be called from either side without leaking storage details into the
// validator. Async because the PG implementation issues queries; the
// in-memory side just wraps a `RwLock<HashMap>` read.
// ---------------------------------------------------------------------------

#[tonic::async_trait]
pub trait MetricLookup: Send + Sync {
    /// Returns true iff *every* id in `metric_ids` exists in the store.
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError>;

    /// Walk a COMPOSITE row and return its direct operand `metric_id`s in
    /// declaration order. Implementations should return `StoreError::NotFound`
    /// when the metric does not exist (the cycle detector treats that as the
    /// "not-yet-inserted root" case and skips the lookup).
    async fn get_composite_operands(&self, metric_id: &str) -> Result<Vec<String>, StoreError>;
}

#[tonic::async_trait]
impl MetricLookup for ManagementStore {
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
        ManagementStore::exists_all_metrics(self, metric_ids).await
    }

    async fn get_composite_operands(&self, metric_id: &str) -> Result<Vec<String>, StoreError> {
        ManagementStore::get_composite_operands(self, metric_id).await
    }
}

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
pub async fn validate_metric_definition<L: MetricLookup + ?Sized>(
    m: &MetricDefinition,
    lookup: &L,
) -> Result<(), Box<Status>> {
    validate_metric_common_fields(m)?;
    match m.r#type() {
        MetricType::FilteredMean => validate_filtered_mean(filtered_mean_cfg(m)),
        MetricType::Composite => {
            validate_composite(composite_cfg(m), &m.metric_id, lookup).await
        }
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

fn composite_cfg(m: &MetricDefinition) -> Option<&CompositeConfig> {
    match m.type_config.as_ref()? {
        MetricTypeConfig::Composite(cfg) => Some(cfg),
        _ => None,
    }
}

#[allow(clippy::result_large_err)]
fn validate_filtered_mean(_cfg: Option<&FilteredMeanConfig>) -> Result<(), Box<Status>> {
    // Filled in by B3 (value_column regex + filter_sql allowlist parser).
    Ok(())
}

// ---------------------------------------------------------------------------
// WINDOWED_COUNT validator (ADR-026 Phase 1, B2)
// ---------------------------------------------------------------------------
//
// Rules enforced here:
//   * `windowed_count` oneof arm must be set (callers that pick
//     `MetricType::WindowedCount` but leave `type_config` empty are misusing
//     the API).
//   * `event_type`: non-empty AND matches the identifier regex
//     `^[a-z_][a-z0-9_]*$`. We do not consult an event catalog — locked in by
//     the plan's "Defaults" section: regex-only until a catalog service
//     exists.
//   * `window_hours`: in `(0, 8760]`. 8760 = 24 * 365 = 1 year cap.
//   * `filter_sql`: optional (empty string is the proto3 default and means
//     "no filter"). When non-empty, B3's allowlist parser is the source of
//     truth. Until B3 lands, we fall back to a 4096-byte sanity bound so the
//     field cannot be abused to push arbitrarily large payloads through the
//     gRPC handler. C1's end-to-end test will catch the gap once B3 ships.

const WINDOWED_COUNT_MAX_HOURS: i32 = 8760;
const FILTER_SQL_MAX_LEN_FALLBACK: usize = 4096;

#[allow(clippy::result_large_err)]
fn validate_windowed_count(cfg: Option<&WindowedCountConfig>) -> Result<(), Box<Status>> {
    let cfg = cfg.ok_or_else(|| {
        Box::new(Status::invalid_argument(
            "windowed_count metric requires WindowedCountConfig",
        ))
    })?;

    if cfg.event_type.is_empty() {
        return Err(Box::new(Status::invalid_argument(
            "windowed_count.event_type must not be empty",
        )));
    }
    if !identifier_re().is_match(&cfg.event_type) {
        return Err(Box::new(Status::invalid_argument(format!(
            "windowed_count.event_type must match identifier regex ^[a-z_][a-z0-9_]*$, got {}",
            cfg.event_type
        ))));
    }

    if cfg.window_hours <= 0 {
        return Err(Box::new(Status::invalid_argument(
            "windowed_count.window_hours must be > 0",
        )));
    }
    if cfg.window_hours > WINDOWED_COUNT_MAX_HOURS {
        return Err(Box::new(Status::invalid_argument(
            "windowed_count.window_hours must be <= 8760 (1 year)",
        )));
    }

    if !cfg.filter_sql.is_empty() {
        // TODO(B3): replace with filter_sql allowlist parser
        // (validators::filter_sql::validate_filter_sql). Until B3 lands, the
        // bound below is a sanity guard, not a real semantic check.
        if cfg.filter_sql.len() >= FILTER_SQL_MAX_LEN_FALLBACK {
            return Err(Box::new(Status::invalid_argument(format!(
                "windowed_count.filter_sql length must be < {} bytes (got {})",
                FILTER_SQL_MAX_LEN_FALLBACK,
                cfg.filter_sql.len()
            ))));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// COMPOSITE validator (ADR-026 Phase 1, B1)
// ---------------------------------------------------------------------------
//
// Rules enforced here:
//   * `composite` oneof arm must be set (callers that pick `MetricType::Composite`
//     but leave `type_config` empty are misusing the API).
//   * `operator` must not be UNSPECIFIED.
//   * Operand arity:
//       ADD / MULTIPLY / WEIGHTED_SUM: >= 2 operands
//       SUBTRACT / DIVIDE:             exactly 2 operands
//   * WEIGHTED_SUM operands must all have `weight > 0`. Other operators ignore
//     the weight field (we do not require it to be zero — proto3 has no way to
//     mean "unset" for scalars and clients legitimately leave it at default).
//   * Every operand `metric_id` exists in the store (single roundtrip via
//     `exists_all_metrics`).
//   * No reference cycle, walked via `composite_cycle::check_no_cycles` with a
//     hard depth cap of 5 (see `DEFAULT_DEPTH_CAP`).

const DEFAULT_DEPTH_CAP: usize = 5;

#[allow(clippy::result_large_err)]
async fn validate_composite<L: MetricLookup + ?Sized>(
    cfg: Option<&CompositeConfig>,
    this_metric_id: &str,
    lookup: &L,
) -> Result<(), Box<Status>> {
    let cfg = cfg.ok_or_else(|| {
        Box::new(Status::invalid_argument(
            "COMPOSITE metric requires composite config (type_config.composite)",
        ))
    })?;

    let op = CompositeOperator::try_from(cfg.operator).unwrap_or(CompositeOperator::Unspecified);
    if op == CompositeOperator::Unspecified {
        return Err(Box::new(Status::invalid_argument(
            "COMPOSITE metric requires a valid operator (got UNSPECIFIED)",
        )));
    }

    let n = cfg.operands.len();
    match op {
        CompositeOperator::Add | CompositeOperator::Multiply | CompositeOperator::WeightedSum => {
            if n < 2 {
                return Err(Box::new(Status::invalid_argument(format!(
                    "COMPOSITE {:?} requires at least 2 operands (got {})",
                    op, n
                ))));
            }
        }
        CompositeOperator::Subtract | CompositeOperator::Divide => {
            if n != 2 {
                return Err(Box::new(Status::invalid_argument(format!(
                    "COMPOSITE {:?} requires exactly 2 operands (got {})",
                    op, n
                ))));
            }
        }
        CompositeOperator::Unspecified => unreachable!("already rejected above"),
    }

    // WEIGHTED_SUM: every operand needs a strictly-positive weight.
    if op == CompositeOperator::WeightedSum {
        for operand in &cfg.operands {
            if !operand.weight.is_finite() || operand.weight <= 0.0 {
                return Err(Box::new(Status::invalid_argument(format!(
                    "COMPOSITE WEIGHTED_SUM operand '{}' has invalid weight {} (must be > 0)",
                    operand.metric_id, operand.weight
                ))));
            }
        }
    }

    // Reject empty / self-referential operand ids before any store round trips.
    for operand in &cfg.operands {
        if operand.metric_id.trim().is_empty() {
            return Err(Box::new(Status::invalid_argument(
                "COMPOSITE operand metric_id must not be empty",
            )));
        }
    }

    // Operand existence — single round trip.
    let operand_ids: Vec<&str> = cfg.operands.iter().map(|o| o.metric_id.as_str()).collect();
    let all_present = lookup
        .exists_all_metrics(&operand_ids)
        .await
        .map_err(store_err_to_status)?;
    if !all_present {
        return Err(Box::new(Status::invalid_argument(format!(
            "COMPOSITE metric '{}' references operands that do not exist",
            this_metric_id
        ))));
    }

    // Cycle + depth cap.
    let owned_ids: Vec<String> = cfg.operands.iter().map(|o| o.metric_id.clone()).collect();
    composite_cycle::check_no_cycles(this_metric_id, &owned_ids, lookup, DEFAULT_DEPTH_CAP).await?;

    Ok(())
}

#[allow(clippy::result_large_err)]
fn store_err_to_status(e: StoreError) -> Box<Status> {
    Box::new(Status::internal(format!(
        "metric lookup failed: {}",
        e
    )))
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
    // MetricDefinition validators (ADR-026 Phase 1)
    // -----------------------------------------------------------------------

    use experimentation_proto::experimentation::common::v1::{
        CompositeOperand, CompositeOperator, FilteredMeanConfig, WindowedCountConfig,
    };
    use std::collections::HashMap;

    fn make_metric(metric_id: &str, name: &str, t: MetricType) -> MetricDefinition {
        MetricDefinition {
            metric_id: metric_id.into(),
            name: name.into(),
            r#type: t as i32,
            ..Default::default()
        }
    }

    /// Map-backed `MetricLookup` for unit tests.
    ///
    /// `graph[id]` lists the direct operands of `id`. A leaf metric simply
    /// has an empty list. Any id present in `graph` is considered to "exist"
    /// for `exists_all_metrics`.
    pub(super) struct MockLookup {
        pub(super) graph: HashMap<String, Vec<String>>,
    }

    impl MockLookup {
        pub(super) fn new(graph: HashMap<String, Vec<String>>) -> Self {
            Self { graph }
        }

        pub(super) fn with_leaves(ids: &[&str]) -> Self {
            let mut g = HashMap::new();
            for id in ids {
                g.insert((*id).to_string(), Vec::new());
            }
            Self::new(g)
        }
    }

    #[tonic::async_trait]
    impl MetricLookup for MockLookup {
        async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
            Ok(metric_ids.iter().all(|id| self.graph.contains_key(*id)))
        }

        async fn get_composite_operands(
            &self,
            metric_id: &str,
        ) -> Result<Vec<String>, StoreError> {
            self.graph
                .get(metric_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(metric_id.to_string()))
        }
    }

    fn empty_lookup() -> MockLookup {
        MockLookup::new(HashMap::new())
    }

    #[tokio::test]
    async fn metric_common_fields_reject_empty_metric_id() {
        let m = make_metric("", "Watch time", MetricType::Mean);
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("metric_id"));
    }

    #[tokio::test]
    async fn metric_common_fields_reject_empty_name() {
        let m = make_metric("watch_time", "", MetricType::Mean);
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("name"));
    }

    #[tokio::test]
    async fn metric_common_fields_reject_whitespace_only() {
        let m = make_metric("   ", "ok", MetricType::Mean);
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert!(err.message().contains("metric_id"));
    }

    #[tokio::test]
    async fn metric_legacy_mean_passes_through() {
        let m = make_metric("watch_time", "Watch time", MetricType::Mean);
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn metric_filtered_mean_skeleton_passes() {
        // B3 fills in the value_column + filter_sql rules.
        let mut m = make_metric("filtered_dur", "Filtered duration", MetricType::FilteredMean);
        m.type_config = Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql: "platform = 'mobile'".into(),
            value_column: "duration_ms".into(),
        }));
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn metric_windowed_count_skeleton_passes() {
        // B2 fills in event_type regex + window_hours range.
        let mut m = make_metric("first_signup", "First signup", MetricType::WindowedCount);
        m.type_config = Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
            event_type: "signup_completed".into(),
            filter_sql: String::new(),
            window_hours: 24,
        }));
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    // ---- COMPOSITE validator (B1) -----------------------------------------

    fn composite_metric(
        metric_id: &str,
        operator: CompositeOperator,
        operands: Vec<(&str, f64)>,
    ) -> MetricDefinition {
        let operands = operands
            .into_iter()
            .map(|(id, w)| CompositeOperand { metric_id: id.into(), weight: w })
            .collect();
        let mut m = make_metric(metric_id, "composite", MetricType::Composite);
        m.type_config = Some(MetricTypeConfig::Composite(CompositeConfig {
            operator: operator as i32,
            operands,
        }));
        m
    }

    #[tokio::test]
    async fn composite_rejects_missing_config() {
        let mut m = make_metric("c", "c", MetricType::Composite);
        m.type_config = None;
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("composite"));
    }

    #[tokio::test]
    async fn composite_rejects_unspecified_operator() {
        let m = composite_metric("c", CompositeOperator::Unspecified, vec![("a", 0.0), ("b", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("UNSPECIFIED"));
    }

    #[tokio::test]
    async fn composite_add_rejects_one_operand() {
        let m = composite_metric("c", CompositeOperator::Add, vec![("a", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("at least 2"));
    }

    #[tokio::test]
    async fn composite_add_accepts_two_operands() {
        let m = composite_metric("c", CompositeOperator::Add, vec![("a", 0.0), ("b", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        assert!(validate_metric_definition(&m, &lookup).await.is_ok());
    }

    #[tokio::test]
    async fn composite_subtract_rejects_three_operands() {
        let m = composite_metric(
            "c",
            CompositeOperator::Subtract,
            vec![("a", 0.0), ("b", 0.0), ("d", 0.0)],
        );
        let lookup = MockLookup::with_leaves(&["a", "b", "d"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("exactly 2"));
    }

    #[tokio::test]
    async fn composite_divide_accepts_two_operands() {
        let m = composite_metric("c", CompositeOperator::Divide, vec![("a", 0.0), ("b", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        assert!(validate_metric_definition(&m, &lookup).await.is_ok());
    }

    #[tokio::test]
    async fn composite_weighted_sum_rejects_zero_weight() {
        let m = composite_metric(
            "c",
            CompositeOperator::WeightedSum,
            vec![("a", 0.0), ("b", 1.0)],
        );
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("weight"));
    }

    #[tokio::test]
    async fn composite_weighted_sum_rejects_negative_weight() {
        let m = composite_metric(
            "c",
            CompositeOperator::WeightedSum,
            vec![("a", -0.5), ("b", 1.0)],
        );
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("weight"));
    }

    #[tokio::test]
    async fn composite_weighted_sum_accepts_positive_weights() {
        let m = composite_metric(
            "c",
            CompositeOperator::WeightedSum,
            vec![("a", 1.5), ("b", 0.25)],
        );
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        assert!(validate_metric_definition(&m, &lookup).await.is_ok());
    }

    #[tokio::test]
    async fn composite_multiply_ignores_weight_field() {
        // Non-WEIGHTED_SUM operators do not read `weight`; zeros must not reject.
        let m = composite_metric("c", CompositeOperator::Multiply, vec![("a", 0.0), ("b", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a", "b"]);
        assert!(validate_metric_definition(&m, &lookup).await.is_ok());
    }

    #[tokio::test]
    async fn composite_rejects_missing_operand() {
        let m = composite_metric("c", CompositeOperator::Add, vec![("a", 0.0), ("ghost", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("operand"));
    }

    #[tokio::test]
    async fn composite_rejects_empty_operand_id() {
        let m = composite_metric("c", CompositeOperator::Add, vec![("a", 0.0), ("", 0.0)]);
        let lookup = MockLookup::with_leaves(&["a"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("empty"));
    }

    #[tokio::test]
    async fn composite_detects_self_reference() {
        let m = composite_metric("c", CompositeOperator::Add, vec![("c", 0.0), ("a", 0.0)]);
        let mut graph = HashMap::new();
        graph.insert("c".to_string(), vec!["c".to_string(), "a".to_string()]);
        graph.insert("a".to_string(), Vec::new());
        let lookup = MockLookup::new(graph);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(err.message().contains("cycle"));
    }

    // ---- WINDOWED_COUNT validator (B2) ------------------------------------

    fn windowed_count_metric(
        event_type: &str,
        window_hours: i32,
        filter_sql: &str,
    ) -> MetricDefinition {
        let mut m = make_metric("wc", "wc", MetricType::WindowedCount);
        m.type_config = Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
            event_type: event_type.into(),
            filter_sql: filter_sql.into(),
            window_hours,
        }));
        m
    }

    #[tokio::test]
    async fn windowed_count_missing_config_rejected() {
        let mut m = make_metric("wc", "wc", MetricType::WindowedCount);
        m.type_config = None;
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("WindowedCountConfig"));
    }

    #[tokio::test]
    async fn windowed_count_empty_event_type_rejected() {
        let m = windowed_count_metric("", 24, "");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("event_type"));
        assert!(err.message().contains("empty"));
    }

    #[tokio::test]
    async fn windowed_count_invalid_event_type_rejected() {
        // Each sub-case violates the identifier regex differently.
        for bad in ["Signup", "1signup", "signup completed", "signup-completed", "SIGNUP"] {
            let m = windowed_count_metric(bad, 24, "");
            let err = validate_metric_definition(&m, &empty_lookup())
                .await
                .unwrap_err_or_else_panic(bad);
            assert_eq!(err.code(), tonic::Code::InvalidArgument, "expected reject for {bad:?}");
            assert!(
                err.message().contains("identifier regex"),
                "message for {bad:?} missing identifier-regex hint: {}",
                err.message()
            );
            assert!(
                err.message().contains(bad),
                "message for {bad:?} should echo offending value: {}",
                err.message()
            );
        }
    }

    #[tokio::test]
    async fn windowed_count_valid_event_type_accepted() {
        for good in ["signup", "page_view", "c1_clicked", "_internal", "a"] {
            let m = windowed_count_metric(good, 24, "");
            assert!(
                validate_metric_definition(&m, &empty_lookup()).await.is_ok(),
                "expected accept for {good:?}"
            );
        }
    }

    #[tokio::test]
    async fn windowed_count_zero_hours_rejected() {
        let m = windowed_count_metric("signup", 0, "");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("window_hours"));
        assert!(err.message().contains("> 0"));
    }

    #[tokio::test]
    async fn windowed_count_negative_hours_rejected() {
        let m = windowed_count_metric("signup", -1, "");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("window_hours"));
        assert!(err.message().contains("> 0"));
    }

    #[tokio::test]
    async fn windowed_count_excessive_hours_rejected() {
        let m = windowed_count_metric("signup", 8761, "");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("window_hours"));
        assert!(err.message().contains("8760"));
    }

    #[tokio::test]
    async fn windowed_count_one_year_accepted() {
        let m = windowed_count_metric("signup", 8760, "");
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn windowed_count_one_hour_accepted() {
        let m = windowed_count_metric("signup", 1, "");
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn windowed_count_with_filter_sql_within_size_bound_accepted() {
        // Short filter; B3 will tighten this with the allowlist parser later.
        let m = windowed_count_metric("page_view", 24, "platform = 'mobile'");
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn windowed_count_with_oversized_filter_sql_rejected() {
        // 4096-byte sanity bound (fallback until B3 lands). Exactly at the
        // cap is rejected; anything below is accepted.
        let big = "a".repeat(FILTER_SQL_MAX_LEN_FALLBACK);
        let m = windowed_count_metric("page_view", 24, &big);
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("filter_sql"));
    }

    // Small helper: panic with a descriptive message inside a loop sub-case.
    // (`Result::unwrap_err` swallows the loop index, which makes failures
    // hard to read.)
    trait UnwrapErrOrPanic<T, E> {
        fn unwrap_err_or_else_panic(self, ctx: &str) -> E;
    }
    impl<T: std::fmt::Debug, E> UnwrapErrOrPanic<T, E> for Result<T, E> {
        fn unwrap_err_or_else_panic(self, ctx: &str) -> E {
            match self {
                Ok(v) => panic!("expected Err for {ctx:?}, got Ok({v:?})"),
                Err(e) => e,
            }
        }
    }
}
