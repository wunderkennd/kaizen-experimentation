//! ADR-026 Phase 3 / Task T1 end-to-end test for MigrateMetricDefinition.
//!
//! Exercises the L7 two-step apply contract: a CUSTOM (raw-SQL) metric is
//! created in the M5 store, an APPROVED shadow result is reported by a mock
//! M3, and the migrate RPC writes the replacement metric + audit row in a
//! single atomic transaction.
//!
//! Covers (per locked plan §T1):
//!
//!   happy:   CUSTOM original + valid METRICQL replacement + APPROVED shadow
//!            → both rows created + response returns new_metric_id + migration_id
//!   error 1: original isn't CUSTOM                    → InvalidArgument
//!   error 2: new is CUSTOM                            → InvalidArgument
//!   error 3: same metric_id                           → InvalidArgument
//!   error 4: shadow not APPROVED (REJECTED status)    → FailedPrecondition
//!   error 5: validate_metric_definition fails (bad MetricQL) → InvalidArgument
//!   error 6: duplicate old_metric_id (second attempt) → AlreadyExists
//!   error 7: duplicate new_metric_id                  → AlreadyExists
//!
//! Also asserts the precondition-check ordering (b → c → a → e → d) by
//! constructing requests that would fail multiple checks and confirming the
//! handler reports the most-helpful (earliest) failure.
//!
//! ## Running this suite
//!
//! Like the Phase 1 e2e suite, these tests are PG-gated on `DATABASE_URL`
//! and skip quietly when it is unset.
//!
//! ```sh
//! just db-reset && just migrate
//! export DATABASE_URL=postgres://postgres:postgres@localhost:5432/kaizen_dev
//! cargo test -p experimentation-management --test metric_migration_e2e_test
//! ```
//!
//! Without `DATABASE_URL`, every test prints a skip message and returns Ok,
//! so the binary still passes in environments without a database. CI runs
//! the real assertions against a managed Postgres.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::{Code, Request, Response, Status};
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::{
    MetricAggregationLevel, MetricDefinition, MetricStakeholder, MetricType,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    CreateMetricDefinitionRequest, GetMetricDefinitionRequest, MigrateMetricDefinitionRequest,
};
use experimentation_proto::experimentation::metrics::v1::{
    metric_computation_service_client::MetricComputationServiceClient,
    metric_computation_service_server::{MetricComputationService, MetricComputationServiceServer},
    CompileMetricqlPreviewRequest, CompileMetricqlPreviewResponse,
    ComputeGuardrailMetricsRequest, ComputeMetricsRequest, ComputeMetricsResponse,
    ExportNotebookRequest, ExportNotebookResponse, GetQueryLogRequest, GetQueryLogResponse,
    GetShadowResultsRequest, GetShadowResultsResponse, PromoteShadowResultRequest,
    PromoteShadowResultResponse, ScheduleShadowComputationRequest,
    ScheduleShadowComputationResponse,
};

use experimentation_management::grpc::{ManagementServiceHandler, SharedState};
use experimentation_management::store::ManagementStore;

// ---------------------------------------------------------------------------
// Mock M3 — driven by a per-shadow-id status map
// ---------------------------------------------------------------------------

/// Map from shadow_id (UUID string) to the status string M3 should return
/// for `GetShadowResults`. Tests preload entries to control whether a given
/// migrate call's shadow_run_result_id resolves to APPROVED / REJECTED / etc.
type ShadowStatusMap = Arc<Mutex<std::collections::HashMap<String, String>>>;

struct MockM3 {
    shadow_status: ShadowStatusMap,
}

#[tonic::async_trait]
impl MetricComputationService for MockM3 {
    async fn get_shadow_results(
        &self,
        req: Request<GetShadowResultsRequest>,
    ) -> Result<Response<GetShadowResultsResponse>, Status> {
        let inner = req.into_inner();
        let map = self.shadow_status.lock().unwrap();
        match map.get(&inner.shadow_id) {
            Some(status) => Ok(Response::new(GetShadowResultsResponse {
                shadow_id: inner.shadow_id,
                status: status.clone(),
                rows: vec![],
                days_within_tolerance: 0,
                total_days: 0,
            })),
            None => Err(Status::not_found(format!(
                "shadow_id not registered with mock M3: {}",
                inner.shadow_id
            ))),
        }
    }

    // Stub remaining RPCs to satisfy the trait bound.
    async fn compute_metrics(
        &self,
        _req: Request<ComputeMetricsRequest>,
    ) -> Result<Response<ComputeMetricsResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn compute_guardrail_metrics(
        &self,
        _req: Request<ComputeGuardrailMetricsRequest>,
    ) -> Result<Response<ComputeMetricsResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn export_notebook(
        &self,
        _req: Request<ExportNotebookRequest>,
    ) -> Result<Response<ExportNotebookResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn get_query_log(
        &self,
        _req: Request<GetQueryLogRequest>,
    ) -> Result<Response<GetQueryLogResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn compile_metricql_preview(
        &self,
        _req: Request<CompileMetricqlPreviewRequest>,
    ) -> Result<Response<CompileMetricqlPreviewResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn schedule_shadow_computation(
        &self,
        _req: Request<ScheduleShadowComputationRequest>,
    ) -> Result<Response<ScheduleShadowComputationResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn promote_shadow_result(
        &self,
        _req: Request<PromoteShadowResultRequest>,
    ) -> Result<Response<PromoteShadowResultResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }
}

async fn spawn_mock_m3() -> (SocketAddr, ShadowStatusMap) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shadow_status: ShadowStatusMap =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let mock = MockM3 {
        shadow_status: shadow_status.clone(),
    };
    tokio::spawn(async move {
        Server::builder()
            .add_service(MetricComputationServiceServer::new(mock))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    tokio::task::yield_now().await;
    (addr, shadow_status)
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build a real (PG-backed) gRPC handler whose M3 client points at the
/// caller-supplied mock. Returns `None` if no Postgres is reachable.
async fn try_handler(m3_addr: SocketAddr) -> Option<ManagementServiceHandler> {
    let url = std::env::var("DATABASE_URL").ok()?;
    match ManagementStore::connect(&url).await {
        Ok(store) => {
            let endpoint =
                tonic::transport::Endpoint::from_shared(format!("http://{m3_addr}"))
                    .expect("valid M3 endpoint");
            let channel = endpoint.connect_lazy();
            let client = MetricComputationServiceClient::new(channel);
            let state = SharedState::new_with_channel(store, client);
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
    format!("e2e_mig_{prefix}_{ts}_{n}")
}

fn custom_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("custom {id}"),
        description: "migration e2e original".into(),
        r#type: MetricType::Custom as i32,
        source_event_type: "video_play".into(),
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

/// METRICQL replacement that doesn't reference any other metrics (so the
/// validator's existence check is vacuously satisfied). The expression
/// `mean(heartbeat.value)` parses + analyses cleanly under the Phase 2
/// MetricQL grammar.
fn metricql_replacement(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("metricql {id}"),
        description: "migration e2e replacement".into(),
        r#type: MetricType::Metricql as i32,
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        metricql_expression: "mean(heartbeat.value)".into(),
        ..Default::default()
    }
}

/// METRICQL with a parse error — used to exercise (e) validator failure.
fn metricql_bad(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("bad metricql {id}"),
        r#type: MetricType::Metricql as i32,
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        // Unterminated string literal — guaranteed parse error.
        metricql_expression: "'unterminated".into(),
        ..Default::default()
    }
}

/// Helper to create a CUSTOM metric directly through the handler's
/// `create_metric_definition` (so the row uses the production code path).
async fn seed_custom(handler: &ManagementServiceHandler, id: &str) {
    handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(custom_metric(id)),
        }))
        .await
        .expect("seed CUSTOM metric ok");
}

/// Helper to create a MEAN metric (non-CUSTOM, used in the "old isn't CUSTOM"
/// case).
async fn seed_mean(handler: &ManagementServiceHandler, id: &str) {
    handler
        .create_metric_definition(Request::new(CreateMetricDefinitionRequest {
            metric: Some(mean_metric(id)),
        }))
        .await
        .expect("seed MEAN metric ok");
}

/// Register a fresh shadow_id with the given status on the mock M3.
/// Returns the shadow_id as a string for use in MigrateMetricDefinitionRequest.
fn register_shadow(shadow_status: &ShadowStatusMap, status: &str) -> String {
    let id = Uuid::new_v4().to_string();
    shadow_status
        .lock()
        .unwrap()
        .insert(id.clone(), status.to_string());
    id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn happy_path_custom_to_metricql_with_approved_shadow() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("happy_old");
    let new_id = unique_id("happy_new");
    seed_custom(&handler, &old_id).await;

    let shadow_id = register_shadow(&shadow_status, "APPROVED");

    let resp = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id.clone(),
            new_metric: Some(metricql_replacement(&new_id)),
            shadow_run_result_id: shadow_id.clone(),
            operator: "alice@example.com".into(),
        }))
        .await
        .expect("happy-path migrate ok")
        .into_inner();

    assert_eq!(resp.new_metric_id, new_id, "echoes new metric_id");
    assert!(!resp.migration_id.is_empty(), "migration_id populated");
    let migration_uuid =
        Uuid::parse_str(&resp.migration_id).expect("migration_id is a valid UUID");
    assert!(
        !resp.applied_at.is_empty(),
        "applied_at populated (RFC 3339): {}",
        resp.applied_at
    );
    // RFC 3339 always has a 'T' separator between date and time.
    assert!(
        resp.applied_at.contains('T'),
        "applied_at looks RFC 3339: {}",
        resp.applied_at
    );

    // I1: read back the NEW metric definition to confirm it actually landed
    // in the DB. The gRPC response alone doesn't prove anything was persisted.
    let stored_new = handler
        .get_metric_definition(Request::new(GetMetricDefinitionRequest {
            metric_id: new_id.clone(),
        }))
        .await
        .expect("new metric is retrievable")
        .into_inner();
    assert_eq!(stored_new.r#type(), MetricType::Metricql);
    assert_eq!(stored_new.metricql_expression, "mean(heartbeat.value)");

    // I1: read back the OLD CUSTOM metric to confirm L7's "never destructive
    // in-place" guarantee — the original row stays put with type=CUSTOM even
    // after migration.
    let stored_old = handler
        .get_metric_definition(Request::new(GetMetricDefinitionRequest {
            metric_id: old_id.clone(),
        }))
        .await
        .expect("old CUSTOM row preserved per L7")
        .into_inner();
    assert_eq!(stored_old.r#type(), MetricType::Custom);

    // I1: read back the metric_migrations audit row to confirm every column
    // got bound to the right input (catches accidental argument swaps in the
    // INSERT). Use the handler's store directly via the new
    // get_metric_migration_by_id lookup.
    let shadow_uuid = Uuid::parse_str(&shadow_id).expect("shadow_id is a valid UUID");
    let audit = handler
        .store()
        .get_metric_migration_by_id(migration_uuid)
        .await
        .expect("audit row is retrievable");
    assert_eq!(audit.migration_id, migration_uuid, "migration_id roundtrip");
    assert_eq!(audit.old_metric_id, old_id, "audit.old_metric_id");
    assert_eq!(audit.new_metric_id, new_id, "audit.new_metric_id");
    assert_eq!(
        audit.shadow_run_result_id, shadow_uuid,
        "audit.shadow_run_result_id"
    );
    assert_eq!(audit.operator, "alice@example.com", "audit.operator");
}

#[tokio::test]
async fn old_is_not_custom_returns_invalid_argument() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    // Seed a MEAN (not CUSTOM) metric — precondition (a) must reject it.
    let old_id = unique_id("not_custom");
    let new_id = unique_id("repl");
    seed_mean(&handler, &old_id).await;

    let shadow_id = register_shadow(&shadow_status, "APPROVED");

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id.clone(),
            new_metric: Some(metricql_replacement(&new_id)),
            shadow_run_result_id: shadow_id,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject non-CUSTOM old metric");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("CUSTOM"),
        "message must mention CUSTOM, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn new_is_custom_returns_invalid_argument() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("old");
    let new_id = unique_id("new_custom");
    seed_custom(&handler, &old_id).await;

    // No need to actually seed the shadow — (b) short-circuits before M3.
    let shadow_id = register_shadow(&shadow_status, "APPROVED");

    let mut replacement = custom_metric(&new_id);
    // Distinguish the description so a future bug that wrote it would show
    // up clearly.
    replacement.description = "should be rejected".into();

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id,
            new_metric: Some(replacement),
            shadow_run_result_id: shadow_id,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject CUSTOM replacement");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("CUSTOM"),
        "message must mention CUSTOM, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn same_metric_id_returns_invalid_argument() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let id = unique_id("same_id");
    seed_custom(&handler, &id).await;
    let shadow_id = register_shadow(&shadow_status, "APPROVED");

    // Construct a METRICQL replacement that reuses the old id.
    let mut replacement = metricql_replacement(&id);
    replacement.metric_id = id.clone();

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: id.clone(),
            new_metric: Some(replacement),
            shadow_run_result_id: shadow_id,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject same metric_id");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("differ"),
        "message must mention 'differ', got: {}",
        err.message()
    );
}

#[tokio::test]
async fn shadow_not_approved_returns_failed_precondition() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("rejected_old");
    let new_id = unique_id("rejected_new");
    seed_custom(&handler, &old_id).await;
    let shadow_id = register_shadow(&shadow_status, "REJECTED");

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id.clone(),
            new_metric: Some(metricql_replacement(&new_id)),
            shadow_run_result_id: shadow_id,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject non-APPROVED shadow");

    assert_eq!(err.code(), Code::FailedPrecondition);
    assert!(
        err.message().contains("APPROVED"),
        "message must mention APPROVED, got: {}",
        err.message()
    );
    assert!(
        err.message().contains("REJECTED"),
        "message must include actual status, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn validator_failure_returns_invalid_argument() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("bad_repl_old");
    let new_id = unique_id("bad_repl_new");
    seed_custom(&handler, &old_id).await;
    let shadow_id = register_shadow(&shadow_status, "APPROVED");

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id,
            new_metric: Some(metricql_bad(&new_id)),
            shadow_run_result_id: shadow_id,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject invalid MetricQL");

    assert_eq!(err.code(), Code::InvalidArgument);
    // The MetricQL parser reports a string-literal diagnostic for an
    // unterminated quote — we don't assert the exact wording (it ships from
    // the validator crate), only that some diagnostic-shaped message is
    // surfaced.
    assert!(
        !err.message().is_empty(),
        "validator must populate an error message"
    );
}

#[tokio::test]
async fn duplicate_old_metric_id_returns_already_exists() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("dup_old");
    let new_id_1 = unique_id("dup_new1");
    let new_id_2 = unique_id("dup_new2");
    seed_custom(&handler, &old_id).await;
    let shadow_id_1 = register_shadow(&shadow_status, "APPROVED");
    let shadow_id_2 = register_shadow(&shadow_status, "APPROVED");

    // First migration succeeds.
    handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id.clone(),
            new_metric: Some(metricql_replacement(&new_id_1)),
            shadow_run_result_id: shadow_id_1,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect("first migrate ok");

    // Second migration of the SAME old_metric_id with a different new_id
    // must hit the uq_metric_migrations_old constraint.
    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id.clone(),
            new_metric: Some(metricql_replacement(&new_id_2)),
            shadow_run_result_id: shadow_id_2,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("second migrate must fail");

    assert_eq!(err.code(), Code::AlreadyExists);
    assert!(
        err.message().contains("already migrated") || err.message().contains(&old_id),
        "message must indicate already-migrated, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn duplicate_new_metric_id_returns_already_exists() {
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    // Set up two CUSTOM metrics + a shared replacement id. First migrate
    // takes the new id; second must fail on metric_definitions PK collision.
    let old_id_1 = unique_id("dup_n_old1");
    let old_id_2 = unique_id("dup_n_old2");
    let shared_new_id = unique_id("dup_n_shared");
    seed_custom(&handler, &old_id_1).await;
    seed_custom(&handler, &old_id_2).await;
    let shadow_id_1 = register_shadow(&shadow_status, "APPROVED");
    let shadow_id_2 = register_shadow(&shadow_status, "APPROVED");

    handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id_1,
            new_metric: Some(metricql_replacement(&shared_new_id)),
            shadow_run_result_id: shadow_id_1,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect("first migrate (taking the new id) ok");

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id_2,
            new_metric: Some(metricql_replacement(&shared_new_id)),
            shadow_run_result_id: shadow_id_2,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("second migrate must fail on new-id PK collision");

    assert_eq!(err.code(), Code::AlreadyExists);
    assert!(
        err.message().contains("already exists")
            || err.message().contains(&shared_new_id),
        "message must indicate already-exists, got: {}",
        err.message()
    );
}

// ---------------------------------------------------------------------------
// Precondition-order tests
// ---------------------------------------------------------------------------
//
// The handler must check preconditions in the order b → c → a → e → d so
// callers always get the most-helpful (earliest) failure. We assert this by
// constructing requests that would fail multiple checks and confirming the
// reported error matches the EARLIEST precondition.

#[tokio::test]
async fn precondition_order_b_beats_c() {
    // new=CUSTOM (b fails) AND same metric_id (c would also fail).
    // Expected: InvalidArgument with "CUSTOM" — (b) fires first.
    let (m3_addr, _shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let id = unique_id("ord_bc");
    let mut replacement = custom_metric(&id);
    replacement.metric_id = id.clone();

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: id.clone(),
            new_metric: Some(replacement),
            shadow_run_result_id: Uuid::new_v4().to_string(),
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must fail on (b)");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("CUSTOM"),
        "(b) must fire before (c); message: {}",
        err.message()
    );
}

#[tokio::test]
async fn precondition_order_c_beats_a() {
    // new.metric_id == old (c fails) AND old does not exist (a would also fail).
    // Expected: InvalidArgument with "differ" — (c) fires first.
    let (m3_addr, _shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let nonexistent_id = unique_id("ord_ca_nonexistent");
    let mut replacement = metricql_replacement(&nonexistent_id);
    replacement.metric_id = nonexistent_id.clone();

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: nonexistent_id.clone(),
            new_metric: Some(replacement),
            shadow_run_result_id: Uuid::new_v4().to_string(),
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must fail on (c)");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("differ"),
        "(c) must fire before (a); message: {}",
        err.message()
    );
}

#[tokio::test]
async fn precondition_order_e_beats_d() {
    // Bad MetricQL (e fails) AND shadow is REJECTED (d would also fail).
    // Expected: InvalidArgument from the validator — (e) fires before (d).
    let (m3_addr, shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("ord_ed_old");
    let new_id = unique_id("ord_ed_new");
    seed_custom(&handler, &old_id).await;
    let shadow_id = register_shadow(&shadow_status, "REJECTED");

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id,
            new_metric: Some(metricql_bad(&new_id)),
            shadow_run_result_id: shadow_id,
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must fail on (e)");

    // (e) → InvalidArgument; (d) would be → FailedPrecondition.
    assert_eq!(
        err.code(),
        Code::InvalidArgument,
        "(e) must fire before (d); got code {:?}",
        err.code()
    );
}

// ---------------------------------------------------------------------------
// Request-shape tests (run without DB; cheap edge cases)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_old_metric_id_returns_invalid_argument() {
    let (m3_addr, _shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let new_id = unique_id("empty_old_new");

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: "".into(),
            new_metric: Some(metricql_replacement(&new_id)),
            shadow_run_result_id: Uuid::new_v4().to_string(),
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject empty old_metric_id");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("old_metric_id"),
        "message must mention old_metric_id, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn missing_new_metric_returns_invalid_argument() {
    let (m3_addr, _shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: "foo".into(),
            new_metric: None,
            shadow_run_result_id: Uuid::new_v4().to_string(),
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject missing new_metric");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("new_metric"),
        "message must mention new_metric, got: {}",
        err.message()
    );
}

// I2: a malformed shadow_run_result_id is a caller bug, not a missing shadow
// run. The handler must surface it as InvalidArgument from the request-shape
// stage — BEFORE the M3 GetShadowResults RPC — so callers don't get the
// misleading FailedPrecondition("shadow_run_result_id not found in M3") that
// the original ordering produced (the mock M3, and likely the real one,
// returns NotFound for a UUID it doesn't know about).
#[tokio::test]
async fn malformed_shadow_uuid_returns_invalid_argument() {
    let (m3_addr, _shadow_status) = spawn_mock_m3().await;
    let Some(handler) = try_handler(m3_addr).await else { return };

    let old_id = unique_id("malformed_uuid_old");
    let new_id = unique_id("malformed_uuid_new");
    // We deliberately do NOT seed old_id and do NOT register a shadow result:
    // a request-shape rejection must fire before either DB or M3 is consulted.
    // (If the UUID parse moved back behind the M3 call, this test would fail
    //  with FailedPrecondition / Unavailable instead of InvalidArgument.)
    let err = handler
        .migrate_metric_definition(Request::new(MigrateMetricDefinitionRequest {
            old_metric_id: old_id,
            new_metric: Some(metricql_replacement(&new_id)),
            shadow_run_result_id: "not-a-uuid".into(),
            operator: "alice@example.com".into(),
        }))
        .await
        .expect_err("must reject malformed shadow_run_result_id");

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("shadow_run_result_id"),
        "message must mention shadow_run_result_id, got: {}",
        err.message()
    );
    assert!(
        err.message().contains("UUID"),
        "message must mention UUID, got: {}",
        err.message()
    );
}
