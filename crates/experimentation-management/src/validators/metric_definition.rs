//! Metric-definition validator and MetricLookup trait.
//!
//! Entry point: [`validate_metric_definition`] — called by `create_metric_definition`
//! and `update_metric_definition` RPCs and by the migration tier helpers.
//!
//! Also hosts [`MetricLookup`], the minimal async trait used by cycle-detection
//! (`validators::cycle`) and by this module to perform operand-existence and
//! cycle checks without leaking storage details into the validator.

use std::sync::OnceLock;

use regex::Regex;
use tonic::Status;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, CompositeConfig, CompositeOperator,
    FilteredMeanConfig, MetricDefinition, MetricType, WindowedCountConfig,
};

use crate::store::{ManagementStore, StoreError};

use super::{cycle, filter_sql, metricql};

// ---------------------------------------------------------------------------
// Shared identifier regex (used by B2 WINDOWED_COUNT.event_type and, when B3
// lands, FILTERED_MEAN.value_column). Compiled once via OnceLock.
// ---------------------------------------------------------------------------
pub(super) fn identifier_re() -> &'static Regex {
    static IDENT_RE: OnceLock<Regex> = OnceLock::new();
    IDENT_RE.get_or_init(|| {
        Regex::new(r"^[a-z_][a-z0-9_]*$").expect("identifier regex is a compile-time constant")
    })
}

// ---------------------------------------------------------------------------
// MetricLookup — the minimal surface the cycle-detection validator needs.
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
    /// when the metric does not exist. For non-COMPOSITE types, returns
    /// `Ok(vec![])`.
    async fn get_composite_operands(&self, metric_id: &str) -> Result<Vec<String>, StoreError>;

    /// Return the parsed `@metric_ref` ids from a stored METRICQL metric's
    /// expression. Returns `StoreError::NotFound` if the metric does not exist.
    /// Returns `Ok(vec![])` for non-METRICQL types.
    async fn get_metricql_refs(&self, metric_id: &str) -> Result<Vec<String>, StoreError>;

    /// Cheap metric-type lookup so the cycle dispatcher can pick the right
    /// neighbor source without speculatively calling both getters.
    /// Returns `StoreError::NotFound` if the metric does not exist.
    async fn get_metric_type(&self, metric_id: &str) -> Result<MetricType, StoreError>;
}

#[tonic::async_trait]
impl MetricLookup for ManagementStore {
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
        ManagementStore::exists_all_metrics(self, metric_ids).await
    }

    async fn get_composite_operands(&self, metric_id: &str) -> Result<Vec<String>, StoreError> {
        ManagementStore::get_composite_operands(self, metric_id).await
    }

    async fn get_metricql_refs(&self, metric_id: &str) -> Result<Vec<String>, StoreError> {
        ManagementStore::get_metricql_refs(self, metric_id).await
    }

    async fn get_metric_type(&self, metric_id: &str) -> Result<MetricType, StoreError> {
        ManagementStore::get_metric_type(self, metric_id).await
    }
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
        // ADR-026 Phase 2: MetricQL expression-based metric.
        MetricType::Metricql => {
            validate_metricql_arm(&m.metricql_expression, &m.metric_id, lookup).await
        }
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

// ---------------------------------------------------------------------------
// FILTERED_MEAN validator (ADR-026 Phase 1, B3)
// ---------------------------------------------------------------------------
//
// Rules enforced here:
//   * `filtered_mean` oneof arm must be set (callers that pick
//     `MetricType::FilteredMean` but leave `type_config` empty are misusing
//     the API).
//   * `value_column` is a bare lowercase identifier — same shape as B2's
//     `event_type`. Reuses `filter_sql::is_identifier` so the two paths
//     can't drift.
//   * `filter_sql` is REQUIRED — FILTERED_MEAN with an empty filter is just
//     MEAN; we send the caller to `METRIC_TYPE_MEAN` rather than silently
//     accepting.
//   * `filter_sql` passes the positive allowlist in
//     `filter_sql::validate_filter_sql` (operators / identifiers / literals
//     only; no functions, subqueries, comments, semicolons, LIKE/BETWEEN/etc.).
#[allow(clippy::result_large_err)]
fn validate_filtered_mean(cfg: Option<&FilteredMeanConfig>) -> Result<(), Box<Status>> {
    let cfg = cfg.ok_or_else(|| {
        Box::new(Status::invalid_argument(
            "FILTERED_MEAN metric requires filtered_mean config (type_config.filtered_mean)",
        ))
    })?;

    if cfg.value_column.trim().is_empty() {
        return Err(Box::new(Status::invalid_argument(
            "FILTERED_MEAN requires value_column",
        )));
    }
    if !filter_sql::is_identifier(&cfg.value_column) {
        return Err(Box::new(Status::invalid_argument(format!(
            "FILTERED_MEAN value_column must be a bare lowercase identifier matching ^[a-z_][a-z0-9_]*$ (got '{}')",
            cfg.value_column
        ))));
    }

    if cfg.filter_sql.trim().is_empty() {
        return Err(Box::new(Status::invalid_argument(
            "filter_sql is required for FILTERED_MEAN; use METRIC_TYPE_MEAN if no filter is needed",
        )));
    }

    filter_sql::validate_filter_sql(&cfg.filter_sql)
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
//     "no filter"). When non-empty, B3's positive allowlist parser
//     (`filter_sql::validate_filter_sql`) is the source of truth — operator
//     allowlist, length cap, comment/semicolon/subquery/function-call rejects.

const WINDOWED_COUNT_MAX_HOURS: i32 = 8760;

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
        // Delegate to B3's positive-allowlist parser. Identical semantics to
        // FILTERED_MEAN.filter_sql: operator allowlist (=/!=/</<=/>/>=, AND/OR/
        // NOT/IN/IS NULL/IS NOT NULL), length cap (4096), and explicit rejects
        // for semicolons, comments, function calls, and subqueries.
        filter_sql::validate_filter_sql(&cfg.filter_sql)?;
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
    cycle::check_no_cycles(this_metric_id, &owned_ids, lookup, DEFAULT_DEPTH_CAP).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// METRICQL validator (ADR-026 Phase 2, A7 + A8)
// ---------------------------------------------------------------------------
//
// Rules enforced here:
//   * `metricql_expression` must be non-empty (METRICQL with no expression is
//     misuse — use a concrete type like MEAN or COMPOSITE instead).
//   * The expression must lex, parse, and pass semantic analysis via
//     `metricql::validate_metricql`.
//   * Every `@metric_ref` in the expression must exist in the store (single
//     round trip via `exists_all_metrics`).
//   * No reference cycle — uses `cycle::check_no_cycles` with the parsed refs
//     as direct_operands. A8 generalized neighbor expansion dispatches on
//     metric type, so METRICQL → METRICQL, METRICQL → COMPOSITE, and
//     COMPOSITE → METRICQL chains are all fully detected.

#[allow(clippy::result_large_err)]
async fn validate_metricql_arm<L: MetricLookup + ?Sized>(
    expression: &str,
    this_metric_id: &str,
    lookup: &L,
) -> Result<(), Box<Status>> {
    // Empty expression for METRICQL type is misuse — reject early.
    if expression.trim().is_empty() {
        return Err(Box::new(Status::invalid_argument(
            "METRICQL metric requires non-empty metricql_expression",
        )));
    }

    // Parse with None so we get the refs first without requiring the known-set
    // (option B from plan: parse → check refs exist → cycle-detect).
    let ctx = metricql::ValidateContext { known_metric_ids: None };
    let refs = match metricql::validate_metricql(expression, &ctx) {
        Ok(refs) => refs,
        Err(diags) => {
            return Err(diagnostics_to_status(diags));
        }
    };

    // Existence check — single round trip.
    if !refs.is_empty() {
        let ref_strs: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
        let all_present = lookup
            .exists_all_metrics(&ref_strs)
            .await
            .map_err(store_err_to_status)?;
        if !all_present {
            return Err(Box::new(Status::invalid_argument(format!(
                "METRICQL metric '{}' references metrics that do not exist",
                this_metric_id
            ))));
        }

        // Cycle + depth cap — generalized DFS dispatches on metric type (A8).
        cycle::check_no_cycles(this_metric_id, &refs, lookup, DEFAULT_DEPTH_CAP).await?;
    }

    Ok(())
}

/// Convert a list of internal `Diagnostic`s into a `Box<Status>`.
///
/// Uses summary-only encoding (first / all errors concatenated in the message).
/// The full structured `MetricqlDiagnosticBag` is surfaced via the dedicated
/// `ValidateMetricql` RPC (A9), which returns the bag in the response body.
/// That is the path the M6 editor uses; this path is for CLI tooling and
/// defense-in-depth at write time.
///
/// TODO(#436.x): serialize the full MetricqlDiagnosticBag into Status details
/// for tooling that parses tonic Any payloads (CLI, language servers).
#[allow(clippy::result_large_err)]
pub(super) fn diagnostics_to_status(diags: Vec<metricql::Diagnostic>) -> Box<Status> {
    let summary = if diags.is_empty() {
        "MetricQL validation failed".to_string()
    } else if diags.len() == 1 {
        diags[0].message.clone()
    } else {
        format!(
            "MetricQL validation failed with {} errors: {}",
            diags.len(),
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("; ")
        )
    };
    Box::new(Status::invalid_argument(summary))
}

#[allow(clippy::result_large_err)]
pub(super) fn store_err_to_status(e: StoreError) -> Box<Status> {
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
        CompositeOperand, CompositeOperator, FilteredMeanConfig, MetricType, WindowedCountConfig,
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

        async fn get_metricql_refs(&self, _metric_id: &str) -> Result<Vec<String>, StoreError> {
            // MockLookup has no type awareness — treat all nodes as COMPOSITE
            // (operands via graph). METRICQL-typed neighbor expansion is tested
            // via GraphLookup in cycle.rs.
            Ok(vec![])
        }

        async fn get_metric_type(
            &self,
            metric_id: &str,
        ) -> Result<MetricType, StoreError> {
            if self.graph.contains_key(metric_id) {
                // MockLookup is used for COMPOSITE-flavoured validator tests;
                // advertise all present nodes as COMPOSITE so the DFS calls
                // get_composite_operands when descending (matching A7 test expectations).
                Ok(MetricType::Composite)
            } else {
                Err(StoreError::NotFound(metric_id.to_string()))
            }
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
        // B3 allowlist accepts simple `<ident> = '<literal>'` predicates.
        let m = windowed_count_metric("page_view", 24, "platform = 'mobile'");
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn windowed_count_with_oversized_filter_sql_rejected() {
        // B3 enforces a 4096-char cap on filter_sql via validate_filter_sql.
        // 4097 chars of any otherwise-valid identifier overflows the cap.
        let big = "a".repeat(4097);
        let m = windowed_count_metric("page_view", 24, &big);
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        // B3's message reads "filter_sql exceeds 4096 character limit".
        assert!(
            err.message().contains("4096") || err.message().contains("limit"),
            "expected length-limit error, got: {}",
            err.message()
        );
    }

    #[tokio::test]
    async fn windowed_count_with_disallowed_filter_sql_rejected() {
        // Proves the B2 → B3 wiring: a subquery in WINDOWED_COUNT.filter_sql
        // must be rejected by the same allowlist that guards FILTERED_MEAN.
        // (Length alone — the previous fallback — would not have caught this.)
        let m = windowed_count_metric(
            "page_view",
            24,
            "user_id IN (SELECT id FROM banned_users)",
        );
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(
            err.message().contains("subqueries"),
            "expected subquery rejection from B3, got: {}",
            err.message()
        );
    }

    #[tokio::test]
    async fn windowed_count_with_function_call_filter_sql_rejected() {
        // Second wiring proof: function calls reach the B3 allowlist.
        let m = windowed_count_metric("page_view", 24, "LOWER(country) = 'us'");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(
            err.message().contains("function call"),
            "expected function-call rejection from B3, got: {}",
            err.message()
        );
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

    // ---- FILTERED_MEAN validator (B3) -------------------------------------

    fn filtered_mean_metric(value_column: &str, filter_sql: &str) -> MetricDefinition {
        let mut m = make_metric("fm", "fm", MetricType::FilteredMean);
        m.type_config = Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql: filter_sql.into(),
            value_column: value_column.into(),
        }));
        m
    }

    #[tokio::test]
    async fn filtered_mean_missing_config_rejected() {
        let mut m = make_metric("fm", "fm", MetricType::FilteredMean);
        m.type_config = None;
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("filtered_mean"));
    }

    #[tokio::test]
    async fn filtered_mean_empty_value_column_rejected() {
        let m = filtered_mean_metric("", "platform = 'mobile'");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("value_column"));
    }

    #[tokio::test]
    async fn filtered_mean_invalid_value_column_rejected() {
        for bad in ["Duration_ms", "1col", "duration ms", "duration-ms", "DURATION"] {
            let m = filtered_mean_metric(bad, "platform = 'mobile'");
            let err = validate_metric_definition(&m, &empty_lookup())
                .await
                .unwrap_err_or_else_panic(bad);
            assert_eq!(err.code(), tonic::Code::InvalidArgument, "expected reject for {bad:?}");
            assert!(
                err.message().contains("value_column"),
                "message for {bad:?} should mention value_column: {}",
                err.message()
            );
        }
    }

    #[tokio::test]
    async fn filtered_mean_empty_filter_sql_rejected() {
        let m = filtered_mean_metric("duration_ms", "");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(
            err.message().contains("METRIC_TYPE_MEAN"),
            "empty filter_sql rejection must guide caller to MEAN: {}",
            err.message()
        );
    }

    #[tokio::test]
    async fn filtered_mean_valid_accepts() {
        let m = filtered_mean_metric(
            "duration_ms",
            "platform = 'mobile' AND duration_ms > 5000",
        );
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn filtered_mean_filter_sql_bad_token_rejected() {
        // LIKE is not in the operator allowlist (Phase 1 decision).
        let m = filtered_mean_metric("duration_ms", "platform LIKE 'mobile%'");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(
            err.message().to_ascii_lowercase().contains("disallowed")
                || err.message().contains("LIKE"),
            "expected allowlist-rejection message, got: {}",
            err.message()
        );
    }

    #[tokio::test]
    async fn filtered_mean_filter_sql_subquery_rejected() {
        let m = filtered_mean_metric("duration_ms", "user_id IN (SELECT id FROM users)");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert!(err.message().contains("subqueries"));
    }

    // --- METRICQL validator (A7 / ADR-026 Phase 2) ----------------------------

    fn metricql_metric(metric_id: &str, expression: &str) -> MetricDefinition {
        let mut m = make_metric(metric_id, metric_id, MetricType::Metricql);
        m.metricql_expression = expression.to_string();
        m
    }

    #[tokio::test]
    async fn metricql_rejects_empty_expression() {
        let m = metricql_metric("engagement", "");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("non-empty"));
    }

    #[tokio::test]
    async fn metricql_rejects_whitespace_only_expression() {
        let m = metricql_metric("engagement", "   ");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("non-empty"));
    }

    #[tokio::test]
    async fn metricql_accepts_valid_no_refs() {
        // mean(playtime.seconds) — no @metric_refs, no store round trip needed.
        let m = metricql_metric("first_view", "mean(playtime.seconds)");
        assert!(validate_metric_definition(&m, &empty_lookup()).await.is_ok());
    }

    #[tokio::test]
    async fn metricql_accepts_valid_with_existing_refs() {
        // 0.7 * @watch_time + 0.3 * @ctr — both refs exist in the store.
        let m = metricql_metric("engagement", "0.7 * @watch_time + 0.3 * @ctr");
        let lookup = MockLookup::with_leaves(&["watch_time", "ctr"]);
        assert!(validate_metric_definition(&m, &lookup).await.is_ok());
    }

    #[tokio::test]
    async fn metricql_rejects_parse_error() {
        // mean(heartbeat.value — unclosed paren. Parser will reject this.
        let m = metricql_metric("broken", "mean(heartbeat.value");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        // Don't assert on exact wording — parser owns it. Just confirm it's not a panic.
    }

    #[tokio::test]
    async fn metricql_rejects_missing_refs() {
        // With empty_lookup all refs are missing → existence check fails.
        let m = metricql_metric("engagement", "0.7 * @watch_time + 0.3 * @ctr");
        let err = validate_metric_definition(&m, &empty_lookup()).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("do not exist"));
    }

    #[tokio::test]
    async fn metricql_rejects_missing_ref_with_lookup() {
        // More specific: watch_time exists, ctr does not.
        let m = metricql_metric("engagement", "0.7 * @watch_time + 0.3 * @ctr");
        let lookup = MockLookup::with_leaves(&["watch_time"]);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("do not exist"));
    }

    #[tokio::test]
    async fn metricql_rejects_simple_cycle() {
        // METRICQL 'me' references @other, where MockLookup treats 'other'
        // as a COMPOSITE-flavoured node with operand 'me' (simulating a cycle).
        // composite_cycle::check_no_cycles catches this via the DFS.
        let m = metricql_metric("me", "1.0 * @other");
        let mut graph = HashMap::new();
        graph.insert("me".to_string(), Vec::new());
        graph.insert("other".to_string(), vec!["me".to_string()]);
        let lookup = MockLookup::new(graph);
        let err = validate_metric_definition(&m, &lookup).await.unwrap_err();
        assert!(
            err.message().to_lowercase().contains("cycle"),
            "expected cycle error, got: {}",
            err.message()
        );
    }
}
