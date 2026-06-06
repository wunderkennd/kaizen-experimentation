//! Contract test for M5 PreviewMetricDefinition → M3 CompileMetricqlPreview proxy.
//!
//! Verifies (ADR-026 Phase 2 / #436, updated for #597 global-scope relaxation):
//!  1. Request forwarding — M5 forwards experiment_id + metricql_expression to M3 unmodified.
//!  2. Response passthrough — compiled_sql returned verbatim.
//!  3. Diagnostics propagated unmodified (field values, span coords).
//!  4. M3 returns a non-OK status → the status code propagates.
//!  5. M3 unavailable → UNAVAILABLE (connect_lazy fails at first send).
//!  6. Empty experiment_id → forwarded to M3 as global-scope (#597, symmetric
//!     to PR #595's validate_metricql relaxation); no longer short-circuited.
//!  7. Empty metricql_expression → INVALID_ARGUMENT before any M3 round-trip.
//!  8. Whitespace-only experiment_id → forwarded to M3 verbatim (#597 — the
//!     trim().is_empty() check is treated as the global-scope signal).
//!
//! ## Design
//!
//! Uses an in-process tonic mock server for M3 (no real M3 instance required).
//! The production `ManagementServiceHandler` is wired with a gRPC channel
//! pointing at the mock server.
//!
//! `ManagementStore` is constructed with sqlx `connect_lazy` — the pool
//! never establishes a TCP connection unless a query is actually executed.
//! `preview_metric_definition` never touches the store, so this is safe.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::{Code, Request, Response, Status};

use experimentation_proto::experimentation::common::v1::{
    metricql_diagnostic::{Severity, Span},
    MetricqlDiagnostic,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    PreviewMetricDefinitionRequest,
};
use experimentation_proto::experimentation::metrics::v1::{
    metric_computation_service_client::MetricComputationServiceClient,
    metric_computation_service_server::{MetricComputationService, MetricComputationServiceServer},
    CompileMetricqlPreviewRequest, CompileMetricqlPreviewResponse, ComputeGuardrailMetricsRequest,
    ComputeMetricsRequest, ComputeMetricsResponse, ExportNotebookRequest, ExportNotebookResponse,
    GetQueryLogRequest, GetQueryLogResponse, GetShadowResultsRequest, GetShadowResultsResponse,
    PromoteShadowResultRequest, PromoteShadowResultResponse, ScheduleShadowComputationRequest,
    ScheduleShadowComputationResponse,
};

use experimentation_management::grpc::{ManagementServiceHandler, SharedState};
use experimentation_management::store::ManagementStore;

// ---------------------------------------------------------------------------
// Mock M3 Server
// ---------------------------------------------------------------------------

/// What the mock captured from its most recent CompileMetricqlPreview call.
#[derive(Debug, Clone)]
struct CapturedCall {
    pub experiment_id: String,
    pub metricql_expression: String,
}

/// Configurable mock M3 service.
struct MockM3 {
    captured: Arc<Mutex<Option<CapturedCall>>>,
    preset_response: Arc<Mutex<PresetResponse>>,
}

/// Clonable preset that the mock returns.
enum PresetResponse {
    Ok(CompileMetricqlPreviewResponse),
    Err(Code, String),
}

impl PresetResponse {
    fn to_result(&self) -> Result<CompileMetricqlPreviewResponse, Status> {
        match self {
            PresetResponse::Ok(r) => Ok(CompileMetricqlPreviewResponse {
                compiled_sql: r.compiled_sql.clone(),
                diagnostics: r.diagnostics.clone(),
            }),
            PresetResponse::Err(code, msg) => Err(Status::new(*code, msg.clone())),
        }
    }
}

#[tonic::async_trait]
impl MetricComputationService for MockM3 {
    async fn compile_metricql_preview(
        &self,
        req: Request<CompileMetricqlPreviewRequest>,
    ) -> Result<Response<CompileMetricqlPreviewResponse>, Status> {
        let inner = req.into_inner();
        *self.captured.lock().unwrap() = Some(CapturedCall {
            experiment_id: inner.experiment_id.clone(),
            metricql_expression: inner.metricql_expression.clone(),
        });
        self.preset_response.lock().unwrap().to_result().map(Response::new)
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

    // ADR-026 Phase 3 / Phase B shadow-run RPCs. Not exercised by the
    // CompileMetricqlPreview contract test; stub to satisfy the trait bound.
    async fn schedule_shadow_computation(
        &self,
        _req: Request<ScheduleShadowComputationRequest>,
    ) -> Result<Response<ScheduleShadowComputationResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn get_shadow_results(
        &self,
        _req: Request<GetShadowResultsRequest>,
    ) -> Result<Response<GetShadowResultsResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }

    async fn promote_shadow_result(
        &self,
        _req: Request<PromoteShadowResultRequest>,
    ) -> Result<Response<PromoteShadowResultResponse>, Status> {
        Err(Status::unimplemented("stub"))
    }
}

// ---------------------------------------------------------------------------
// Helper: spawn mock M3 server and return its bound address + shared state.
// ---------------------------------------------------------------------------

async fn spawn_mock_m3(
    preset: PresetResponse,
) -> (
    SocketAddr,
    Arc<Mutex<Option<CapturedCall>>>,
    Arc<Mutex<PresetResponse>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let captured: Arc<Mutex<Option<CapturedCall>>> = Arc::new(Mutex::new(None));
    let response = Arc::new(Mutex::new(preset));

    let mock = MockM3 {
        captured: captured.clone(),
        preset_response: response.clone(),
    };

    tokio::spawn(async move {
        Server::builder()
            .add_service(MetricComputationServiceServer::new(mock))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    // Yield so the server is ready before the first client call.
    tokio::task::yield_now().await;

    (addr, captured, response)
}

// ---------------------------------------------------------------------------
// Helper: build a production ManagementServiceHandler with a metrics_client
// pointing at `metrics_addr`.
//
// ManagementStore is built with connect_lazy so no PostgreSQL connection is
// attempted.  preview_metric_definition never reads from the store, so this
// is safe for these contract tests.
// ---------------------------------------------------------------------------

async fn make_handler(metrics_addr: String) -> ManagementServiceHandler {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgresql://postgres:postgres@127.0.0.1:1/nonexistent")
        .expect("connect_lazy should not dial");
    let store = ManagementStore::from_pool(pool);

    let endpoint = tonic::transport::Endpoint::from_shared(metrics_addr)
        .expect("valid metrics_addr");
    let channel = endpoint.connect_lazy();
    let client = MetricComputationServiceClient::new(channel);

    let state = SharedState::new_with_channel(store, client);
    ManagementServiceHandler::new(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn forwards_experiment_id_and_expression_to_m3_unmodified() {
    let (addr, captured, _resp) = spawn_mock_m3(PresetResponse::Ok(
        CompileMetricqlPreviewResponse {
            compiled_sql: "SELECT avg(v) FROM t".into(),
            diagnostics: vec![],
        },
    ))
    .await;

    let handler = make_handler(format!("http://{}", addr)).await;
    handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "exp-abc".into(),
            metricql_expression: "mean(heartbeat.value)".into(),
        }))
        .await
        .expect("should succeed");

    let call = captured.lock().unwrap().clone().unwrap();
    assert_eq!(call.experiment_id, "exp-abc", "experiment_id must be forwarded");
    assert_eq!(
        call.metricql_expression, "mean(heartbeat.value)",
        "metricql_expression must be forwarded"
    );
}

#[tokio::test]
async fn compiled_sql_returned_unmodified() {
    let expected_sql = "SELECT avg(t.val) FROM delta_table t WHERE t.exp_id = 'exp-1'";
    let (addr, _captured, _resp) = spawn_mock_m3(PresetResponse::Ok(
        CompileMetricqlPreviewResponse {
            compiled_sql: expected_sql.into(),
            diagnostics: vec![],
        },
    ))
    .await;
    let handler = make_handler(format!("http://{}", addr)).await;

    let resp = handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "exp-1".into(),
            metricql_expression: "mean(heartbeat.value)".into(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.compiled_sql, expected_sql);
    assert!(resp.diagnostics.is_empty());
}

#[tokio::test]
async fn diagnostics_propagated_unmodified() {
    let diag = MetricqlDiagnostic {
        severity: Severity::Error as i32,
        message: "unknown metric ref @nonexistent".into(),
        span: Some(Span {
            start_offset: 5,
            end_offset: 18,
            line: 1,
            column: 6,
        }),
    };
    let (addr, _captured, _resp) = spawn_mock_m3(PresetResponse::Ok(
        CompileMetricqlPreviewResponse {
            compiled_sql: String::new(),
            diagnostics: vec![diag],
        },
    ))
    .await;
    let handler = make_handler(format!("http://{}", addr)).await;

    let resp = handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "exp-1".into(),
            metricql_expression: "mean(@nonexistent)".into(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.diagnostics.len(), 1, "exactly one diagnostic expected");
    assert_eq!(
        resp.diagnostics[0].message,
        "unknown metric ref @nonexistent"
    );
    let span = resp.diagnostics[0].span.as_ref().unwrap();
    assert_eq!(span.start_offset, 5);
    assert_eq!(span.end_offset, 18);
    assert_eq!(span.line, 1);
    assert_eq!(span.column, 6);
    assert!(resp.compiled_sql.is_empty());
}

#[tokio::test]
async fn m3_internal_error_propagates_to_caller() {
    let (addr, _captured, _resp) =
        spawn_mock_m3(PresetResponse::Err(Code::Internal, "M3 Spark job failed".into())).await;
    let handler = make_handler(format!("http://{}", addr)).await;

    let err = handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "exp-1".into(),
            metricql_expression: "mean(x.y)".into(),
        }))
        .await
        .unwrap_err();

    assert_eq!(err.code(), Code::Internal, "M3 Internal must propagate");
    assert!(err.message().contains("M3 Spark job failed"));
}

#[tokio::test]
async fn m3_unavailable_returns_unavailable() {
    // Port 1 is reserved; nothing listens there. connect_lazy() won't panic;
    // the first send will fail with a connection error → Unavailable.
    let handler = make_handler("http://127.0.0.1:1".into()).await;

    let err = handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "exp-1".into(),
            metricql_expression: "mean(x.y)".into(),
        }))
        .await
        .unwrap_err();

    assert_eq!(
        err.code(),
        Code::Unavailable,
        "connection refused must surface as Unavailable, got {:?}: {}",
        err.code(),
        err.message()
    );
}

#[tokio::test]
async fn empty_experiment_id_is_forwarded_to_m3_as_global_scope() {
    // #597 (symmetric to PR #595): empty experiment_id is the global-scope
    // signal used by the metric-creation form. M5 must NOT short-circuit
    // with INVALID_ARGUMENT; instead it forwards the empty string verbatim
    // and M3 resolves the catalog itself.
    let (addr, captured, _resp) = spawn_mock_m3(PresetResponse::Ok(
        CompileMetricqlPreviewResponse {
            compiled_sql: "SELECT avg(v) FROM t".into(),
            diagnostics: vec![],
        },
    ))
    .await;
    let handler = make_handler(format!("http://{}", addr)).await;

    handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: String::new(),
            metricql_expression: "mean(x.y)".into(),
        }))
        .await
        .expect("empty experiment_id must proxy through to M3, not short-circuit");

    let call = captured
        .lock()
        .unwrap()
        .clone()
        .expect("M3 must have received the proxied call");
    assert_eq!(
        call.experiment_id, "",
        "empty experiment_id must be forwarded verbatim, not rewritten"
    );
    assert_eq!(call.metricql_expression, "mean(x.y)");
}

#[tokio::test]
async fn empty_metricql_expression_returns_invalid_argument_without_m3_call() {
    let handler = make_handler("http://127.0.0.1:1".into()).await;

    let err = handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "exp-1".into(),
            metricql_expression: "".into(),
        }))
        .await
        .unwrap_err();

    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("metricql_expression"),
        "error message should mention metricql_expression, got: {}",
        err.message()
    );
}

#[tokio::test]
async fn whitespace_only_experiment_id_is_forwarded_to_m3_as_global_scope() {
    // #597: whitespace-only experiment_id is treated identically to empty
    // — trim().is_empty() is the global-scope signal. M5 forwards the
    // string verbatim; M3 trims on its side when building the catalog.
    let (addr, captured, _resp) = spawn_mock_m3(PresetResponse::Ok(
        CompileMetricqlPreviewResponse {
            compiled_sql: "SELECT avg(v) FROM t".into(),
            diagnostics: vec![],
        },
    ))
    .await;
    let handler = make_handler(format!("http://{}", addr)).await;

    handler
        .preview_metric_definition(Request::new(PreviewMetricDefinitionRequest {
            experiment_id: "   \t".into(),
            metricql_expression: "mean(x.y)".into(),
        }))
        .await
        .expect("whitespace-only experiment_id must proxy through to M3, not short-circuit");

    let call = captured
        .lock()
        .unwrap()
        .clone()
        .expect("M3 must have received the proxied call");
    assert_eq!(
        call.experiment_id, "   \t",
        "whitespace-only experiment_id must be forwarded verbatim"
    );
}
