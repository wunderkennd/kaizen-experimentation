//! ADR-026 Phase 3 / Task C1 end-to-end test.
//!
//! Exercises the M5 Rust deprecation surface for CUSTOM metric creates: on a
//! successful `create_metric_definition` for `METRIC_TYPE_CUSTOM`, the server
//! must emit a non-blocking HTTP/2 response **header** `x-kaizen-deprecation`
//! carrying the L5-locked deprecation message. Non-CUSTOM types (MEAN,
//! FILTERED_MEAN, ...) must NOT emit the header.
//!
//! Note on terminology (Devin PR #578 round-1 🚩 fix): the L5 contract is
//! a header, not a gRPC trailer. Tonic's `Response::metadata_mut()` sets
//! initial response metadata (HTTP/2 headers). Application trailers are not
//! part of tonic's unary response surface — only the framework's
//! `grpc-status` / `grpc-message` trailers exist for unary calls. The runbook
//! at `docs/runbooks/m5-metric-definitions.md#custom-deprecation` consistently
//! refers to it as a header; this test now matches.
//!
//! `UpdateMetricDefinition` does not exist as an RPC and per L7 the metric
//! type is immutable post-create, so `CreateMetricDefinition` is the only
//! deprecation entry point covered by this suite.
//!
//! ## Running this suite
//!
//! Like the Phase 1 e2e suite, these tests are PG-gated on `DATABASE_URL` and
//! skip quietly when it is unset.
//!
//! ```sh
//! just db-reset && just migrate
//! export DATABASE_URL=postgres://postgres:postgres@localhost:5432/kaizen_dev
//! cargo test -p experimentation-management --test metric_deprecation_e2e_test
//! ```
//!
//! Without `DATABASE_URL`, every test prints a skip message and returns Ok,
//! so the binary still passes in environments without a database. CI runs
//! the real assertions against a managed Postgres.

use std::sync::atomic::{AtomicU64, Ordering};

use tonic::Request;

use experimentation_management::grpc::{ManagementServiceHandler, SharedState};
use experimentation_management::store::ManagementStore;
use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, FilteredMeanConfig, MetricAggregationLevel,
    MetricDefinition, MetricStakeholder, MetricType,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    CreateMetricDefinitionRequest,
};

// ---------------------------------------------------------------------------
// L5-locked deprecation header (must match the const in grpc.rs byte-for-byte)
// ---------------------------------------------------------------------------

/// L5-locked deprecation header value for CUSTOM metric creates.
/// Mirror of `DEPRECATION_HEADER_CUSTOM` in `src/grpc.rs`. This test fails
/// loudly if the two ever drift.
const DEPRECATION_HEADER_CUSTOM: &str = "kind=metric_type; type=CUSTOM; message=CUSTOM metrics are deprecated in favor of MetricQL or structured types. See docs/runbooks/m5-metric-definitions.md#custom-deprecation for the migration guide.";

const DEPRECATION_HEADER_NAME: &str = "x-kaizen-deprecation";

// ---------------------------------------------------------------------------
// Fixtures (mirror metric_definition_e2e_test.rs)
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
    format!("e2e_dep_{prefix}_{ts}_{n}")
}

fn custom_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("custom {id}"),
        description: "deprecation e2e fixture".into(),
        r#type: MetricType::Custom as i32,
        source_event_type: "video_play".into(),
        // M5's CUSTOM acceptance only requires the common fields plus
        // custom_sql carrying *some* string; the validator does not parse it.
        custom_sql: "SELECT COUNT(*) FROM video_play".into(),
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        ..Default::default()
    }
}

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

fn filtered_mean_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("filtered mean {id}"),
        description: "deprecation e2e fixture".into(),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_custom_emits_deprecation_header() {
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("custom_header");
    let metric = custom_metric(&id);

    let response = handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(metric),
        }))
        .await
        .expect("create_metric_definition CUSTOM ok");

    let header = response
        .metadata()
        .get(DEPRECATION_HEADER_NAME)
        .unwrap_or_else(|| {
            panic!(
                "expected `{DEPRECATION_HEADER_NAME}` metadata on CUSTOM create response"
            )
        });
    let header_str = header
        .to_str()
        .expect("deprecation header is ascii / valid UTF-8");
    assert_eq!(
        header_str, DEPRECATION_HEADER_CUSTOM,
        "deprecation header must match L5-locked value byte-for-byte"
    );
}

#[tokio::test]
async fn create_mean_does_not_emit_deprecation_header() {
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("mean_no_header");
    let metric = mean_metric(&id);

    let response = handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(metric),
        }))
        .await
        .expect("create_metric_definition MEAN ok");

    assert!(
        response.metadata().get(DEPRECATION_HEADER_NAME).is_none(),
        "MEAN create must NOT emit `{DEPRECATION_HEADER_NAME}`; got {:?}",
        response.metadata().get(DEPRECATION_HEADER_NAME)
    );
}

#[tokio::test]
async fn create_filtered_mean_does_not_emit_deprecation_header() {
    let Some(handler) = try_handler().await else { return };
    let id = unique_id("filtered_mean_no_header");
    let metric = filtered_mean_metric(&id);

    let response = handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(metric),
        }))
        .await
        .expect("create_metric_definition FILTERED_MEAN ok");

    assert!(
        response.metadata().get(DEPRECATION_HEADER_NAME).is_none(),
        "FILTERED_MEAN create must NOT emit `{DEPRECATION_HEADER_NAME}`; got {:?}",
        response.metadata().get(DEPRECATION_HEADER_NAME)
    );
}
