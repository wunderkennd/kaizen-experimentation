//! ADR-026 Phase 2 follow-up (#571 Task 1) — global-scope `ValidateMetricql`.
//!
//! These tests exercise the live-lint path used by the M6 metric-creation form,
//! which has no experiment context yet. When `experiment_id` is empty the M5
//! handler must build its `known_metric_ids` set from the global
//! `metric_definitions` table (via `ManagementStore::list_metrics(default)`),
//! NOT reject the request with `INVALID_ARGUMENT`.
//!
//! ## Running this suite
//!
//! Mirrors `metric_definition_e2e_test.rs`: gated on `DATABASE_URL`, skips
//! quietly when unset.
//!
//! ```sh
//! just db-reset && just migrate
//! export DATABASE_URL=postgres://postgres:postgres@localhost:5432/kaizen_dev
//! cargo test -p experimentation-management --test validate_metricql_global_scope_test
//! ```
//!
//! Pure-Rust safety-net unit tests for the same contract live in
//! `crates/experimentation-management/src/grpc.rs` (`mod tests`); see
//! `global_scope_empty_catalog_flags_unknown_ref_with_position` and
//! `global_scope_with_known_id_validates_clean`.

use std::sync::atomic::{AtomicU64, Ordering};

use tonic::Request;

use experimentation_management::grpc::{ManagementServiceHandler, SharedState};
use experimentation_management::store::ManagementStore;
use experimentation_proto::experimentation::common::v1::{
    MetricAggregationLevel, MetricDefinition, MetricStakeholder, MetricType,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    CreateMetricDefinitionRequest, ValidateMetricqlRequest,
};

// ---------------------------------------------------------------------------
// Fixtures (mirror metric_definition_e2e_test.rs)
// ---------------------------------------------------------------------------

async fn try_handler() -> Option<ManagementServiceHandler> {
    let url = std::env::var("DATABASE_URL").ok()?;
    match ManagementStore::connect(&url).await {
        Ok(store) => {
            let state = SharedState::new(store);
            Some(ManagementServiceHandler::new(state))
        }
        Err(e) => {
            eprintln!("skip: could not connect to DATABASE_URL: {e}");
            None
        }
    }
}

/// Process-unique id with a stable prefix so the same metric can be looked up
/// across the two test cases via the global catalog.
fn unique_id(prefix: &str) -> String {
    static CTR: AtomicU64 = AtomicU64::new(1);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("e2e_{prefix}_{ts}_{n}")
}

/// A MEAN metric that will be inserted into `metric_definitions` so the
/// global-scope path of `validate_metricql` can find it.
fn mean_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("mean {id}"),
        r#type: MetricType::Mean as i32,
        source_event_type: "video_play".into(),
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Test 1 — empty experiment_id + known metric → valid
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_experiment_id_with_known_metric_returns_valid() {
    let Some(handler) = try_handler().await else {
        return;
    };

    // Create a metric in the global catalog (no experiment scope).
    let metric_id = unique_id("watch_time");
    handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(mean_metric(&metric_id)),
        }))
        .await
        .expect("create_metric_definition ok");

    // Live-lint path: empty experiment_id, expression references the new metric.
    // We deliberately wrap @id in an arithmetic Composite (`@id + 0`) because the
    // analyzer rejects a *bare* @ref at root as semantically nonsensical for a
    // metric definition; the live-lint surface we care about is the existence
    // check inside a composite expression, which is exactly what the
    // metric-creation form will type.
    let expression = format!("@{metric_id} + 0");
    let response = handler
        .validate_metricql(Request::new(ValidateMetricqlRequest {
            experiment_id: String::new(),
            metricql_expression: expression,
        }))
        .await
        .expect("validate_metricql must accept empty experiment_id")
        .into_inner();

    assert!(
        response.diagnostics.is_empty(),
        "expected no diagnostics for known global metric, got: {:?}",
        response.diagnostics
    );
    assert_eq!(
        response.referenced_metric_ids,
        vec![metric_id],
        "referenced_metric_ids must surface the resolved @ref"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — empty experiment_id + unknown metric → diagnostic with line:col
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_experiment_id_with_unknown_metric_returns_diagnostic() {
    let Some(handler) = try_handler().await else {
        return;
    };

    // A metric_id that we deliberately do NOT insert. Process-unique so any
    // residual seed data in a shared DB cannot satisfy it.
    let unknown_id = unique_id("never_created");
    let expression = format!("@{unknown_id} + 0");
    let response = handler
        .validate_metricql(Request::new(ValidateMetricqlRequest {
            experiment_id: String::new(),
            metricql_expression: expression,
        }))
        .await
        .expect("validate_metricql must accept empty experiment_id")
        .into_inner();

    assert_eq!(
        response.diagnostics.len(),
        1,
        "expected exactly one diagnostic for an unknown global metric, got: {:?}",
        response.diagnostics
    );

    let diag = &response.diagnostics[0];
    let lower = diag.message.to_lowercase();
    assert!(
        lower.contains("unknown") || lower.contains("unresolved") || lower.contains("not found"),
        "diagnostic must flag the unresolved ref; got message: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains(&unknown_id),
        "diagnostic message must mention the unknown metric id {unknown_id}; got: {:?}",
        diag.message
    );

    let span = diag.span.as_ref().expect("diagnostic must carry a span");
    assert!(
        span.line >= 1,
        "diagnostic line must be 1-indexed; got line={}",
        span.line
    );
    assert!(
        span.column >= 1,
        "diagnostic column must be 1-indexed; got column={}",
        span.column
    );

    assert!(
        response.referenced_metric_ids.is_empty(),
        "referenced_metric_ids must be empty when diagnostics fire; got: {:?}",
        response.referenced_metric_ids
    );
}
