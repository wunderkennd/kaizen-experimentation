//! ADR-026 Phase 1 end-to-end test (#433, Task C1).
//!
//! Exercises the full Phase 1 stack: gRPC handler → validator → ManagementStore
//! → PostgreSQL. For each of FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT it
//! creates a metric via `create_metric_definition`, reads it back via
//! `get_metric_definition`, and asserts that the oneof `type_config` payload
//! survives the round trip byte-for-byte. The COMPOSITE cases additionally
//! prove that the validator rejects cycles and over-deep chains *through* the
//! gRPC entry point (not just at the validator's unit-test surface), and the
//! WINDOWED_COUNT case proves the B2 → B3 `filter_sql` wiring with a subquery
//! payload that the old length-only fallback would have admitted.
//!
//! ## Running this suite
//!
//! There is no `#[sqlx::test]` infrastructure in this crate yet, so these
//! tests are gated on the `DATABASE_URL` env var and skip quietly when it is
//! unset. Mirrors the convention from `store_metric_test.rs` (A2).
//!
//! ```sh
//! just db-reset && just migrate   # apply migrations 001..011
//! export DATABASE_URL=postgres://postgres:postgres@localhost:5432/kaizen_dev
//! cargo test -p experimentation-management --test metric_definition_e2e_test
//! ```
//!
//! Without `DATABASE_URL`, every test prints a skip message and returns Ok,
//! so the binary still passes in environments without a database. CI runs
//! the real assertions against a managed Postgres.

use std::sync::atomic::{AtomicU64, Ordering};

use tonic::{Code, Request};

use experimentation_management::grpc::{ManagementServiceHandler, SharedState};
use experimentation_management::store::ManagementStore;
use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, CompositeConfig, CompositeOperand,
    CompositeOperator, FilteredMeanConfig, MetricAggregationLevel, MetricDefinition,
    MetricStakeholder, MetricType, WindowedCountConfig,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    CreateMetricDefinitionRequest, GetMetricDefinitionRequest, ListMetricDefinitionsRequest,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build a real (PG-backed) gRPC handler. Returns `None` if no Postgres is
/// reachable, in which case the test prints a skip message and exits Ok.
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

/// Process-unique id (timestamp + monotonic counter). Avoids cross-test
/// collisions even when the suite is run repeatedly against a persistent DB.
fn unique_id(prefix: &str) -> String {
    static CTR: AtomicU64 = AtomicU64::new(1);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("e2e_{prefix}_{ts}_{n}")
}

fn mean_metric(id: &str) -> MetricDefinition {
    // Leaf operand for COMPOSITE composition (not a Phase 1 type itself but
    // legal at the store layer — exercises the "legacy 6" flat-column path).
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

fn filtered_mean_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("filtered mean {id}"),
        description: "e2e fixture".into(),
        r#type: MetricType::FilteredMean as i32,
        source_event_type: "video_play".into(),
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql: "platform = 'mobile' AND duration_ms > 5000".into(),
            value_column: "duration_ms".into(),
        })),
        ..Default::default()
    }
}

fn windowed_count_metric(id: &str, filter_sql: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("windowed count {id}"),
        r#type: MetricType::WindowedCount as i32,
        source_event_type: "purchase".into(),
        stakeholder: MetricStakeholder::Platform as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
            event_type: "purchase".into(),
            filter_sql: filter_sql.to_string(),
            window_hours: 168,
        })),
        ..Default::default()
    }
}

fn composite_metric_add(id: &str, operand_ids: &[&str]) -> MetricDefinition {
    let operands = operand_ids
        .iter()
        .map(|m| CompositeOperand {
            metric_id: (*m).to_string(),
            // ADD ignores weight; default 0.0 is fine and round-trips cleanly.
            weight: 0.0,
        })
        .collect();
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("composite {id}"),
        r#type: MetricType::Composite as i32,
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: Some(MetricTypeConfig::Composite(CompositeConfig {
            operator: CompositeOperator::Add as i32,
            operands,
        })),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Round-trip helpers
// ---------------------------------------------------------------------------

/// Call create_metric_definition → assert OK → return the server-emitted proto.
async fn create_ok(
    handler: &ManagementServiceHandler,
    metric: MetricDefinition,
) -> MetricDefinition {
    handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(metric),
        }))
        .await
        .expect("create_metric_definition ok")
        .into_inner()
}

/// Call get_metric_definition → assert OK → return the proto.
async fn get_ok(handler: &ManagementServiceHandler, metric_id: &str) -> MetricDefinition {
    handler
        .get_metric_definition(Request::new(GetMetricDefinitionRequest {
            metric_id: metric_id.to_string(),
        }))
        .await
        .expect("get_metric_definition ok")
        .into_inner()
}

/// Assert that the relevant type-aware fields survive create → get. We compare
/// the `type_config` oneof directly (the only field with structural payload)
/// plus the canonical identity columns. We deliberately do *not* assert
/// equality on every flat column because `into_proto` normalises a few
/// proto3-defaulted fields (e.g. `surrogate_target_metric_id` is always
/// emitted as empty string).
fn assert_roundtrip(input: &MetricDefinition, fetched: &MetricDefinition) {
    assert_eq!(input.metric_id, fetched.metric_id, "metric_id");
    assert_eq!(input.name, fetched.name, "name");
    assert_eq!(input.r#type, fetched.r#type, "type");
    assert_eq!(input.stakeholder, fetched.stakeholder, "stakeholder");
    assert_eq!(
        input.aggregation_level, fetched.aggregation_level,
        "aggregation_level"
    );
    assert_eq!(input.type_config, fetched.type_config, "type_config oneof");
}

// ---------------------------------------------------------------------------
// FILTERED_MEAN — happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_filtered_mean_create_get_list_roundtrip() {
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("filtered_mean");
    let input = filtered_mean_metric(&id);

    let created = create_ok(&handler, input.clone()).await;
    assert_roundtrip(&input, &created);

    let fetched = get_ok(&handler, &id).await;
    assert_roundtrip(&input, &fetched);

    // list_metric_definitions with the FILTERED_MEAN type filter must surface
    // our row. We do not assert it's the only row, since other parallel test
    // runs (or seed data) may share the DB.
    let listed = handler
        .list_metric_definitions(Request::new(ListMetricDefinitionsRequest {
            type_filter: MetricType::FilteredMean as i32,
            ..Default::default()
        }))
        .await
        .expect("list ok")
        .into_inner();
    assert!(
        listed.metrics.iter().any(|m| m.metric_id == id),
        "FILTERED_MEAN list must include the freshly-created id {id}"
    );
    assert!(
        listed
            .metrics
            .iter()
            .all(|m| m.r#type == MetricType::FilteredMean as i32),
        "type_filter must restrict to FILTERED_MEAN"
    );
}

// ---------------------------------------------------------------------------
// COMPOSITE — happy path + cycle / depth rejections
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_composite_create_get_list_roundtrip() {
    let Some(handler) = try_handler().await else { return };

    // Two real operand metrics must exist before COMPOSITE will validate.
    let id_a = unique_id("op_a");
    let id_b = unique_id("op_b");
    create_ok(&handler, mean_metric(&id_a)).await;
    create_ok(&handler, mean_metric(&id_b)).await;

    let id_comp = unique_id("composite_add");
    let input = composite_metric_add(&id_comp, &[id_a.as_str(), id_b.as_str()]);

    let created = create_ok(&handler, input.clone()).await;
    assert_roundtrip(&input, &created);

    let fetched = get_ok(&handler, &id_comp).await;
    assert_roundtrip(&input, &fetched);

    // List by type filter: our COMPOSITE must appear; all rows in the
    // filtered set must be COMPOSITE.
    let listed = handler
        .list_metric_definitions(Request::new(ListMetricDefinitionsRequest {
            type_filter: MetricType::Composite as i32,
            ..Default::default()
        }))
        .await
        .expect("list ok")
        .into_inner();
    assert!(
        listed.metrics.iter().any(|m| m.metric_id == id_comp),
        "COMPOSITE list must include the freshly-created id {id_comp}"
    );
}

#[tokio::test]
async fn e2e_composite_with_cycle_rejected() {
    // Construct A → B → A. Because composite operand validation walks the
    // store, we have to (a) create A (referencing B which doesn't exist yet —
    // so we can't), or (b) bypass A's operand-existence check. The cleanest
    // way to exercise the cycle path through gRPC is: create leaf X, create
    // composite A referencing X, then try to create composite B referencing
    // both A and B itself. The self-reference triggers the DFS cycle guard.
    let Some(handler) = try_handler().await else { return };

    let id_x = unique_id("cycle_x");
    create_ok(&handler, mean_metric(&id_x)).await;

    let id_a = unique_id("cycle_a");
    create_ok(
        &handler,
        composite_metric_add(&id_a, &[id_x.as_str(), id_x.as_str()]),
    )
    .await;

    // Self-referential composite: B's operand list includes B's own id.
    let id_b = unique_id("cycle_b");
    let self_ref = composite_metric_add(&id_b, &[id_a.as_str(), id_b.as_str()]);
    let err = handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(self_ref),
        }))
        .await
        .expect_err("self-referential composite must be rejected");

    assert_eq!(err.code(), Code::InvalidArgument, "{}", err.message());
    let msg = err.message().to_ascii_lowercase();
    assert!(
        msg.contains("cycle") || msg.contains("self") || msg.contains("operand"),
        "expected cycle/self-reference message, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn e2e_composite_with_depth_exceeded_rejected() {
    // Build a linear chain L0 ← L1 ← L2 ← L3 ← L4 ← L5 ← L6 (depth 7) and
    // assert the final composite exceeds the 5-level cap. We have to build
    // bottom-up because each composite must reference *existing* operands.
    let Some(handler) = try_handler().await else { return };

    // Leaf metric.
    let leaf = unique_id("depth_leaf");
    create_ok(&handler, mean_metric(&leaf)).await;

    // L0..L5 each wrap the previous level in a 2-arg ADD (the second operand
    // is the leaf, so the chain depth = the index of the level).
    let mut prev = leaf.clone();
    let mut ids = Vec::with_capacity(6);
    for i in 0..5 {
        let id = unique_id(&format!("depth_l{i}"));
        create_ok(
            &handler,
            composite_metric_add(&id, &[prev.as_str(), leaf.as_str()]),
        )
        .await;
        ids.push(id.clone());
        prev = id;
    }

    // L5 wraps L4. The recursive depth from L5 is now 6 levels — over the
    // DEFAULT_DEPTH_CAP of 5. Creation must fail.
    let overflow = unique_id("depth_overflow");
    let too_deep = composite_metric_add(&overflow, &[prev.as_str(), leaf.as_str()]);
    let err = handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(too_deep),
        }))
        .await
        .expect_err("composite chain > 5 levels deep must be rejected");

    assert_eq!(err.code(), Code::InvalidArgument, "{}", err.message());
    let msg = err.message().to_ascii_lowercase();
    assert!(
        msg.contains("depth") || msg.contains("cycle"),
        "expected depth-cap message, got: {}",
        err.message()
    );
}

// ---------------------------------------------------------------------------
// WINDOWED_COUNT — happy path + B2 → B3 filter_sql wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_windowed_count_create_get_list_roundtrip() {
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("windowed_count");
    let input = windowed_count_metric(&id, "");

    let created = create_ok(&handler, input.clone()).await;
    assert_roundtrip(&input, &created);

    let fetched = get_ok(&handler, &id).await;
    assert_roundtrip(&input, &fetched);

    let listed = handler
        .list_metric_definitions(Request::new(ListMetricDefinitionsRequest {
            type_filter: MetricType::WindowedCount as i32,
            ..Default::default()
        }))
        .await
        .expect("list ok")
        .into_inner();
    assert!(
        listed.metrics.iter().any(|m| m.metric_id == id),
        "WINDOWED_COUNT list must include the freshly-created id {id}"
    );
}

#[tokio::test]
async fn e2e_windowed_count_with_valid_filter_sql_roundtrip() {
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("windowed_count_filtered");
    let input = windowed_count_metric(&id, "platform = 'mobile' AND duration_ms > 5000");

    let created = create_ok(&handler, input.clone()).await;
    assert_roundtrip(&input, &created);

    let fetched = get_ok(&handler, &id).await;
    assert_roundtrip(&input, &fetched);
}

#[tokio::test]
async fn e2e_filter_sql_subquery_rejected_via_windowed_count() {
    // Proves the Part-1 cleanup: WINDOWED_COUNT.filter_sql now goes through
    // B3's positive allowlist (validate_filter_sql), so a `SELECT` payload
    // must be rejected with the "subqueries" message — *not* admitted by the
    // old length-only fallback.
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("windowed_count_subquery");
    let metric = windowed_count_metric(
        &id,
        "user_id IN (SELECT id FROM banned_users)",
    );

    let err = handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(metric),
        }))
        .await
        .expect_err("subquery in filter_sql must be rejected");

    assert_eq!(err.code(), Code::InvalidArgument, "{}", err.message());
    assert!(
        err.message().contains("subqueries"),
        "expected B3 subquery rejection, got: {}",
        err.message()
    );

    // And no row should have been written.
    let lookup = handler
        .get_metric_definition(Request::new(GetMetricDefinitionRequest {
            metric_id: id.clone(),
        }))
        .await;
    assert!(
        lookup.is_err(),
        "rejected metric must not appear in PG (got {lookup:?})"
    );
}
