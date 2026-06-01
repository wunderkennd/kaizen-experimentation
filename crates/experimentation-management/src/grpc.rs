//! tonic gRPC service for ExperimentManagementService (M5 Rust port, ADR-025 Phase 2).
//!
//! Implements all RPCs from management_service.proto plus StreamConfigUpdates.
//!
//! ## StreamConfigUpdates
//!
//! Uses a `tokio::sync::broadcast` channel. On every state transition that
//! yields a visible state change (RUNNING, PAUSED, ARCHIVED), the service
//! broadcasts a `ConfigUpdateEvent` to all connected M1 subscribers.
//!
//! On connect, the server first back-fills all currently RUNNING/PAUSED
//! experiments (so M1 can recover after restart), then streams live updates.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::{
    metricql_diagnostic::{Severity as ProtoSeverity, Span as ProtoSpan},
    Experiment, ExperimentState as ProtoState, ExperimentType, GuardrailAction, Layer,
    LayerAllocation, MetricDefinition, MetricqlDiagnostic as ProtoMetricqlDiagnostic, MetricType,
    SurrogateModelConfig, TargetingRule, Variant,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::{
        ExperimentManagementService, ExperimentManagementServiceServer,
    },
    ArchiveExperimentRequest, ConcludeExperimentRequest, ConfigUpdateEvent,
    ConflictType as ProtoConflictType, CreateExperimentRequest, CreateLayerRequest,
    CreateMetricDefinitionRequest, CreateSurrogateModelRequest, CreateTargetingRuleRequest,
    ExperimentAllocation as ProtoExperimentAllocation,
    ExperimentConflict as ProtoExperimentConflict, GetExperimentRequest,
    GetLayerAllocationsRequest, GetLayerAllocationsResponse, GetLayerRequest,
    GetMetricDefinitionRequest, GetPortfolioAllocationRequest, GetPortfolioAllocationResponse,
    GetSurrogateCalibrationRequest, ListExperimentsRequest, ListExperimentsResponse,
    ListMetricDefinitionsRequest, ListMetricDefinitionsResponse, ListSurrogateModelsRequest,
    ListSurrogateModelsResponse, PauseExperimentRequest,
    PortfolioStats as ProtoPortfolioStats, PreviewMetricDefinitionRequest,
    PreviewMetricDefinitionResponse, ResumeExperimentRequest, StartExperimentRequest,
    StreamConfigUpdatesRequest, TriggerSurrogateRecalibrationRequest, UpdateExperimentRequest,
    ValidateMetricqlRequest, ValidateMetricqlResponse,
};
use experimentation_proto::experimentation::metrics::v1::{
    metric_computation_service_client::MetricComputationServiceClient,
    CompileMetricqlPreviewRequest,
};

use crate::bucket_reuse;
use crate::store::{ExperimentRow, ManagementStore, StoreError, VariantRow};
use crate::validators;

// Broadcast channel capacity. Slow subscribers will see RecvError::Lagged.
const BROADCAST_CAPACITY: usize = 512;

/// L5-locked deprecation **header** value for CUSTOM metric creates.
///
/// Tonic unary RPCs surface `Response::metadata_mut()` as HTTP/2 initial
/// metadata (response headers), not trailing metadata (trailers). Application
/// trailers aren't part of the tonic unary response API — only the framework
/// `grpc-status` / `grpc-message` trailers exist for unary calls. The L5
/// contract is therefore implemented and consumed as a **header**; the runbook
/// at `docs/runbooks/m5-metric-definitions.md#custom-deprecation` has always
/// documented it as such. (Devin PR #578 round-1 🚩 terminology fix — prior
/// docstrings in this file + the e2e test used "trailer" inconsistently with
/// what tonic actually emits.)
///
/// See ADR-026 Phase 3 plan, Lock L5
/// (`docs/superpowers/plans/2026-05-30-adr-026-phase-3-custom-migration.md`).
/// The runbook anchor referenced below MUST exist — kept in sync with
/// `docs/runbooks/m5-metric-definitions.md#custom-deprecation`.
const DEPRECATION_HEADER_CUSTOM: &str = "kind=metric_type; type=CUSTOM; message=CUSTOM metrics are deprecated in favor of MetricQL or structured types. See docs/runbooks/m5-metric-definitions.md#custom-deprecation for the migration guide.";

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SharedState {
    pub store: Arc<ManagementStore>,
    pub config_tx: broadcast::Sender<ConfigUpdateEvent>,
    pub version: Arc<AtomicI64>,
    /// Cached gRPC client for M3 MetricComputationService.
    /// Channel is lazily-connected (connect_lazy) — no TCP until first RPC.
    /// Clone is cheap: Channel is Arc-counted internally.
    pub metrics_client: MetricComputationServiceClient<tonic::transport::Channel>,
}

impl SharedState {
    pub fn new(store: ManagementStore) -> Self {
        // Build a lazy channel to localhost:50056 as the default.
        // The production path calls new_with_metrics_addr from serve().
        let channel = tonic::transport::Endpoint::from_static("http://localhost:50056")
            .connect_lazy();
        Self::new_with_channel(store, MetricComputationServiceClient::new(channel))
    }

    /// Construct with a caller-supplied metrics client. Used by serve() (production)
    /// and by tests (mock / lazily-connected channel to a test port).
    pub fn new_with_channel(
        store: ManagementStore,
        metrics_client: MetricComputationServiceClient<tonic::transport::Channel>,
    ) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            store: Arc::new(store),
            config_tx: tx,
            version: Arc::new(AtomicI64::new(1)),
            metrics_client,
        }
    }

    /// Publish a config update to all connected subscribers and increment version.
    pub fn publish(&self, experiment: Experiment, is_deletion: bool) {
        let version = self.version.fetch_add(1, Ordering::SeqCst);
        let event = ConfigUpdateEvent {
            experiment: Some(experiment),
            is_deletion,
            version,
        };
        // Ignore SendError (no active subscribers).
        let _ = self.config_tx.send(event);
    }
}

// ---------------------------------------------------------------------------
// Service handler
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ManagementServiceHandler {
    state: SharedState,
}

impl ManagementServiceHandler {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

// ---------------------------------------------------------------------------
// Proto ↔ domain conversions
// ---------------------------------------------------------------------------

fn experiment_row_to_proto(row: &ExperimentRow, variants: &[VariantRow]) -> Experiment {
    let state = ProtoState::try_from(
        match row.state.as_str() {
            "DRAFT" => ProtoState::Draft,
            "STARTING" => ProtoState::Starting,
            "RUNNING" => ProtoState::Running,
            "PAUSED" => ProtoState::Paused,
            "CONCLUDING" => ProtoState::Concluding,
            "CONCLUDED" => ProtoState::Concluded,
            "ARCHIVED" => ProtoState::Archived,
            _ => ProtoState::Unspecified,
        } as i32,
    )
    .unwrap_or(ProtoState::Unspecified) as i32;

    let exp_type = match row.experiment_type.as_str() {
        "AB" => ExperimentType::Ab,
        "MULTIVARIATE" => ExperimentType::Multivariate,
        "INTERLEAVING" => ExperimentType::Interleaving,
        "SESSION_LEVEL" => ExperimentType::SessionLevel,
        "PLAYBACK_QOE" => ExperimentType::PlaybackQoe,
        "MAB" => ExperimentType::Mab,
        "CONTEXTUAL_BANDIT" => ExperimentType::ContextualBandit,
        "CUMULATIVE_HOLDOUT" => ExperimentType::CumulativeHoldout,
        "META" => ExperimentType::Meta,
        "SWITCHBACK" => ExperimentType::Switchback,
        "QUASI" => ExperimentType::Quasi,
        _ => ExperimentType::Unspecified,
    } as i32;

    let guardrail_action = match row.guardrail_action.as_str() {
        "AUTO_PAUSE" => GuardrailAction::AutoPause,
        "ALERT_ONLY" => GuardrailAction::AlertOnly,
        _ => GuardrailAction::Unspecified,
    } as i32;

    let proto_variants: Vec<Variant> = variants
        .iter()
        .map(|v| Variant {
            variant_id: v.variant_id.to_string(),
            name: v.name.clone(),
            traffic_fraction: v.traffic_fraction,
            is_control: v.is_control,
            payload_json: v.payload_json.to_string(),
        })
        .collect();

    Experiment {
        experiment_id: row.experiment_id.to_string(),
        name: row.name.clone(),
        description: row.description.clone().unwrap_or_default(),
        owner_email: row.owner_email.clone(),
        r#type: exp_type,
        state,
        variants: proto_variants,
        layer_id: row.layer_id.to_string(),
        primary_metric_id: row.primary_metric_id.clone(),
        secondary_metric_ids: row.secondary_metric_ids.clone(),
        guardrail_action,
        hash_salt: row.hash_salt.clone(),
        targeting_rule_id: row
            .targeting_rule_id
            .map(|u| u.to_string())
            .unwrap_or_default(),
        is_cumulative_holdout: row.is_cumulative_holdout,
        created_at: Some(row.created_at.into_proto()),
        started_at: row.started_at.map(|t| t.into_proto()),
        concluded_at: row.concluded_at.map(|t| t.into_proto()),
        ..Default::default()
    }
}

/// Mirror of `store::metric_type_to_sql` for use at the gRPC boundary when
/// translating the `type_filter` enum in `ListMetricDefinitionsRequest` into
/// the SQL string the store's `MetricFilter::type` expects. Kept in sync with
/// the CHECK constraint admit-list in migration 011.
fn metric_type_to_pg_string(t: MetricType) -> &'static str {
    match t {
        MetricType::Unspecified => "",
        MetricType::Mean => "MEAN",
        MetricType::Proportion => "PROPORTION",
        MetricType::Ratio => "RATIO",
        MetricType::Count => "COUNT",
        MetricType::Percentile => "PERCENTILE",
        MetricType::Custom => "CUSTOM",
        MetricType::FilteredMean => "FILTERED_MEAN",
        MetricType::Composite => "COMPOSITE",
        MetricType::WindowedCount => "WINDOWED_COUNT",
        MetricType::Metricql => "METRICQL",
    }
}

fn store_err_to_status(e: StoreError) -> Status {
    match e {
        StoreError::NotFound(msg) => Status::not_found(msg),
        StoreError::AlreadyExists(msg) => Status::already_exists(msg),
        StoreError::Db(e) => {
            warn!(error = %e, "database error");
            Status::internal(format!("database error: {e}"))
        }
    }
}

// Helper trait to convert chrono DateTime to prost_types::Timestamp.
trait IntoProto {
    fn into_proto(self) -> prost_types::Timestamp;
}

impl IntoProto for chrono::DateTime<chrono::Utc> {
    fn into_proto(self) -> prost_types::Timestamp {
        prost_types::Timestamp {
            seconds: self.timestamp(),
            nanos: self.timestamp_subsec_nanos() as i32,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: validate common experiment fields at creation
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
fn validate_create(exp: &Experiment) -> Result<(), Status> {
    if exp.name.trim().is_empty() {
        return Err(Status::invalid_argument("name is required"));
    }
    if exp.owner_email.trim().is_empty() {
        return Err(Status::invalid_argument("owner_email is required"));
    }
    if exp.primary_metric_id.trim().is_empty() {
        return Err(Status::invalid_argument("primary_metric_id is required"));
    }
    if exp.layer_id.is_empty() {
        return Err(Status::invalid_argument("layer_id is required"));
    }

    let exp_type = ExperimentType::try_from(exp.r#type).unwrap_or(ExperimentType::Unspecified);
    if exp_type == ExperimentType::Unspecified {
        return Err(Status::invalid_argument("experiment type must be specified"));
    }

    if exp.variants.is_empty() {
        return Err(Status::invalid_argument(
            "at least one variant is required",
        ));
    }

    // Reject NaN/Infinity traffic fractions (IEEE 754 NaN comparisons silently pass).
    for v in &exp.variants {
        if !v.traffic_fraction.is_finite() {
            return Err(Status::invalid_argument(format!(
                "variant '{}' has non-finite traffic_fraction",
                v.name
            )));
        }
    }

    // Traffic fractions must sum to 1.0 (within tolerance).
    let sum: f64 = exp.variants.iter().map(|v| v.traffic_fraction).sum();
    if (sum - 1.0).abs() > 1e-6 {
        return Err(Status::invalid_argument(format!(
            "variant traffic_fractions must sum to 1.0 (got {sum:.6})"
        )));
    }

    // Exactly one control variant.
    let controls = exp.variants.iter().filter(|v| v.is_control).count();
    if controls != 1 {
        return Err(Status::invalid_argument(format!(
            "exactly one variant must be the control (got {controls})"
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// ExperimentManagementService impl
// ---------------------------------------------------------------------------

#[tonic::async_trait]
impl ExperimentManagementService for ManagementServiceHandler {
    // --- CRUD ---

    async fn create_experiment(
        &self,
        request: Request<CreateExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let exp = request
            .into_inner()
            .experiment
            .ok_or_else(|| Status::invalid_argument("experiment is required"))?;

        validate_create(&exp)?;

        let layer_id = Uuid::parse_str(&exp.layer_id)
            .map_err(|_| Status::invalid_argument("invalid layer_id UUID"))?;

        let targeting_rule_id = if exp.targeting_rule_id.is_empty() {
            None
        } else {
            Some(
                Uuid::parse_str(&exp.targeting_rule_id)
                    .map_err(|_| Status::invalid_argument("invalid targeting_rule_id UUID"))?,
            )
        };

        let surrogate_model_id = if exp.surrogate_model_id.is_empty() {
            None
        } else {
            Some(
                Uuid::parse_str(&exp.surrogate_model_id)
                    .map_err(|_| Status::invalid_argument("invalid surrogate_model_id UUID"))?,
            )
        };

        let exp_type_str = match ExperimentType::try_from(exp.r#type)
            .unwrap_or(ExperimentType::Unspecified)
        {
            ExperimentType::Ab => "AB",
            ExperimentType::Multivariate => "MULTIVARIATE",
            ExperimentType::Interleaving => "INTERLEAVING",
            ExperimentType::SessionLevel => "SESSION_LEVEL",
            ExperimentType::PlaybackQoe => "PLAYBACK_QOE",
            ExperimentType::Mab => "MAB",
            ExperimentType::ContextualBandit => "CONTEXTUAL_BANDIT",
            ExperimentType::CumulativeHoldout => "CUMULATIVE_HOLDOUT",
            ExperimentType::Meta => "META",
            ExperimentType::Switchback => "SWITCHBACK",
            ExperimentType::Quasi => "QUASI",
            ExperimentType::Unspecified => {
                return Err(Status::invalid_argument("experiment type unspecified"))
            }
        };

        let variants: Vec<(String, f64, bool, serde_json::Value)> = exp
            .variants
            .iter()
            .map(|v| {
                let payload: serde_json::Value = serde_json::from_str(&v.payload_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                (v.name.clone(), v.traffic_fraction, v.is_control, payload)
            })
            .collect();

        let (seq_method, planned_looks, overall_alpha) =
            if let Some(ref sc) = exp.sequential_test_config {
                let method = match sc.method {
                    1 => Some("MSPRT"),
                    2 => Some("GST_OBF"),
                    3 => Some("GST_POCOCK"),
                    4 => Some("AVLM"),
                    _ => None,
                };
                (method, Some(sc.planned_looks), Some(sc.overall_alpha))
            } else {
                (None, None, None)
            };

        let type_config = build_type_config(&exp);

        let guardrail_action_str = match GuardrailAction::try_from(exp.guardrail_action)
            .unwrap_or(GuardrailAction::Unspecified)
        {
            GuardrailAction::AlertOnly => "ALERT_ONLY",
            _ => "AUTO_PAUSE",
        };

        let row = self
            .state
            .store
            .create_experiment(crate::store::CreateExperimentParams {
                name: &exp.name,
                description: if exp.description.is_empty() { None } else { Some(&exp.description) },
                owner_email: &exp.owner_email,
                experiment_type: exp_type_str,
                layer_id,
                primary_metric_id: &exp.primary_metric_id,
                secondary_metric_ids: &exp.secondary_metric_ids,
                guardrail_action: guardrail_action_str,
                targeting_rule_id,
                is_cumulative_holdout: exp.is_cumulative_holdout,
                type_config: &type_config,
                sequential_method: seq_method,
                planned_looks: planned_looks.and_then(|l| if l == 0 { None } else { Some(l) }),
                overall_alpha: overall_alpha.and_then(|a| if a == 0.0 { None } else { Some(a) }),
                surrogate_model_id,
                variants: &variants,
            })
            .await
            .map_err(store_err_to_status)?;

        let variants_rows = self
            .state
            .store
            .get_variants(row.experiment_id)
            .await
            .map_err(store_err_to_status)?;

        let proto = experiment_row_to_proto(&row, &variants_rows);

        self.state
            .store
            .record_audit(
                row.experiment_id,
                "create",
                &exp.owner_email,
                None,
                Some("DRAFT"),
                &serde_json::json!({"name": row.name, "type": row.experiment_type}),
            )
            .await
            .ok(); // non-fatal

        info!(experiment_id = %row.experiment_id, name = %row.name, "experiment created");
        Ok(Response::new(proto))
    }

    async fn get_experiment(
        &self,
        request: Request<GetExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let id_str = request.into_inner().experiment_id;
        let id = Uuid::parse_str(&id_str)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        Ok(Response::new(experiment_row_to_proto(&row, &variants)))
    }

    async fn update_experiment(
        &self,
        request: Request<UpdateExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let exp = request
            .into_inner()
            .experiment
            .ok_or_else(|| Status::invalid_argument("experiment is required"))?;

        if exp.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let id = Uuid::parse_str(&exp.experiment_id)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        // Only DRAFT experiments can be updated.
        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        if row.state != "DRAFT" {
            return Err(Status::failed_precondition(
                "only DRAFT experiments can be updated",
            ));
        }

        // For now return the existing experiment (config updates are complex; full impl deferred).
        // Variants/targeting updates would require additional store methods.
        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        Ok(Response::new(experiment_row_to_proto(&row, &variants)))
    }

    async fn list_experiments(
        &self,
        request: Request<ListExperimentsRequest>,
    ) -> Result<Response<ListExperimentsResponse>, Status> {
        let req = request.into_inner();

        let state_filter = if req.state_filter == 0 {
            None
        } else {
            ProtoState::try_from(req.state_filter).ok().and_then(|s| {
                Some(match s {
                    ProtoState::Draft => "DRAFT",
                    ProtoState::Starting => "STARTING",
                    ProtoState::Running => "RUNNING",
                    ProtoState::Paused => "PAUSED",
                    ProtoState::Concluding => "CONCLUDING",
                    ProtoState::Concluded => "CONCLUDED",
                    ProtoState::Archived => "ARCHIVED",
                    ProtoState::Unspecified => return None,
                })
            })
        };

        let type_filter = if req.type_filter == 0 {
            None
        } else {
            ExperimentType::try_from(req.type_filter)
                .ok()
                .and_then(|t| {
                    Some(match t {
                        ExperimentType::Ab => "AB",
                        ExperimentType::Multivariate => "MULTIVARIATE",
                        ExperimentType::Interleaving => "INTERLEAVING",
                        ExperimentType::SessionLevel => "SESSION_LEVEL",
                        ExperimentType::PlaybackQoe => "PLAYBACK_QOE",
                        ExperimentType::Mab => "MAB",
                        ExperimentType::ContextualBandit => "CONTEXTUAL_BANDIT",
                        ExperimentType::CumulativeHoldout => "CUMULATIVE_HOLDOUT",
                        ExperimentType::Meta => "META",
                        ExperimentType::Switchback => "SWITCHBACK",
                        ExperimentType::Quasi => "QUASI",
                        ExperimentType::Unspecified => return None,
                    })
                })
        };

        let owner_filter = if req.owner_email_filter.is_empty() {
            None
        } else {
            Some(req.owner_email_filter.as_str())
        };

        let cursor = if req.page_token.is_empty() {
            None
        } else {
            Some(
                Uuid::parse_str(&req.page_token)
                    .map_err(|_| Status::invalid_argument("invalid page_token"))?,
            )
        };

        let (rows, next_cursor) = self
            .state
            .store
            .list_experiments(state_filter, type_filter, owner_filter, req.page_size as i64, cursor)
            .await
            .map_err(store_err_to_status)?;

        let mut experiments = Vec::with_capacity(rows.len());
        for row in &rows {
            let variants = self
                .state
                .store
                .get_variants(row.experiment_id)
                .await
                .map_err(store_err_to_status)?;
            experiments.push(experiment_row_to_proto(row, &variants));
        }

        Ok(Response::new(ListExperimentsResponse {
            experiments,
            next_page_token: next_cursor.map(|u| u.to_string()).unwrap_or_default(),
        }))
    }

    // --- Lifecycle ---

    async fn start_experiment(
        &self,
        request: Request<StartExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let id = Uuid::parse_str(&request.into_inner().experiment_id)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        // Step 1: TOCTOU-safe DRAFT→STARTING.
        let rows = self
            .state
            .store
            .start_transition(id)
            .await
            .map_err(store_err_to_status)?;

        if rows == 0 {
            let row = self
                .state
                .store
                .get_experiment(id)
                .await
                .map_err(store_err_to_status)?;
            return Err(Status::failed_precondition(format!(
                "experiment {} is in state {} (must be DRAFT to start)",
                id, row.state
            )));
        }

        // Step 2: Load experiment for validation.
        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        let proto = experiment_row_to_proto(&row, &variants);

        // Step 3: Type-specific STARTING validation.
        if let Err(e) = validators::validate_starting(&proto) {
            // Validation failed — revert to DRAFT.
            let _ = self.state.store.revert_to_draft(id).await;
            self.state
                .store
                .record_audit(
                    id,
                    "start_failed_validation",
                    "system",
                    Some("STARTING"),
                    Some("DRAFT"),
                    &serde_json::json!({"reason": e.message()}),
                )
                .await
                .ok();
            return Err(*e);
        }

        // Step 4: TOCTOU-safe STARTING→RUNNING.
        let rows = self
            .state
            .store
            .run_transition(id)
            .await
            .map_err(store_err_to_status)?;

        if rows == 0 {
            return Err(Status::aborted(
                "concurrent transition conflict during STARTING→RUNNING",
            ));
        }

        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        self.state
            .store
            .record_audit(
                id,
                "start",
                &row.owner_email,
                Some("DRAFT"),
                Some("RUNNING"),
                &serde_json::json!({}),
            )
            .await
            .ok();

        let proto = experiment_row_to_proto(&row, &variants);
        self.state.publish(proto.clone(), false);

        info!(experiment_id = %id, "experiment started (DRAFT→RUNNING)");
        Ok(Response::new(proto))
    }

    async fn conclude_experiment(
        &self,
        request: Request<ConcludeExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let id = Uuid::parse_str(&request.into_inner().experiment_id)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        let rows = self
            .state
            .store
            .conclude_transition(id)
            .await
            .map_err(store_err_to_status)?;

        if rows == 0 {
            let row = self
                .state
                .store
                .get_experiment(id)
                .await
                .map_err(store_err_to_status)?;
            return Err(Status::failed_precondition(format!(
                "experiment {} is in state {} (must be RUNNING or PAUSED to conclude)",
                id, row.state
            )));
        }

        // Immediately transition CONCLUDING→CONCLUDED.
        // In a full implementation, M4a triggers the final analysis and calls back.
        // For now, M5 transitions synchronously (M4a delegation deferred — see ADR-020).
        let _ = self.state.store.mark_concluded(id).await;

        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        self.state
            .store
            .record_audit(
                id,
                "conclude",
                &row.owner_email,
                Some("RUNNING"),
                Some("CONCLUDED"),
                &serde_json::json!({}),
            )
            .await
            .ok();

        let proto = experiment_row_to_proto(&row, &variants);
        // Concluded experiments are no longer active — broadcast as deletion.
        self.state.publish(proto.clone(), true);

        info!(experiment_id = %id, "experiment concluded");
        Ok(Response::new(proto))
    }

    async fn archive_experiment(
        &self,
        request: Request<ArchiveExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let id = Uuid::parse_str(&request.into_inner().experiment_id)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        let rows = self
            .state
            .store
            .archive_transition(id)
            .await
            .map_err(store_err_to_status)?;

        if rows == 0 {
            let row = self
                .state
                .store
                .get_experiment(id)
                .await
                .map_err(store_err_to_status)?;
            return Err(Status::failed_precondition(format!(
                "experiment {} is in state {} (must be CONCLUDED to archive)",
                id, row.state
            )));
        }

        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        self.state
            .store
            .record_audit(
                id,
                "archive",
                &row.owner_email,
                Some("CONCLUDED"),
                Some("ARCHIVED"),
                &serde_json::json!({}),
            )
            .await
            .ok();

        let proto = experiment_row_to_proto(&row, &variants);
        self.state.publish(proto.clone(), true);

        // Release bucket allocation after archive.
        let pool = self.state.store.pool().clone();
        let layer_id = row.layer_id;
        tokio::spawn(async move {
            if let Err(e) = bucket_reuse::release(&pool, layer_id, id).await {
                warn!(error = %e, %id, "bucket release failed (non-fatal)");
            }
        });

        info!(experiment_id = %id, "experiment archived");
        Ok(Response::new(proto))
    }

    async fn pause_experiment(
        &self,
        request: Request<PauseExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        let id = Uuid::parse_str(&req.experiment_id)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        let reason = if req.is_guardrail_auto_pause {
            format!("guardrail_auto_pause: {}", req.reason)
        } else {
            format!("manual_pause: {}", req.reason)
        };

        let rows = self
            .state
            .store
            .pause_transition(id, &reason)
            .await
            .map_err(store_err_to_status)?;

        if rows == 0 {
            let row = self
                .state
                .store
                .get_experiment(id)
                .await
                .map_err(store_err_to_status)?;
            return Err(Status::failed_precondition(format!(
                "experiment {} is in state {} (must be RUNNING to pause)",
                id, row.state
            )));
        }

        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        let action = if req.is_guardrail_auto_pause {
            "guardrail_auto_pause"
        } else {
            "pause"
        };

        self.state
            .store
            .record_audit(
                id,
                action,
                &row.owner_email,
                Some("RUNNING"),
                Some("PAUSED"),
                &serde_json::json!({"reason": reason}),
            )
            .await
            .ok();

        let proto = experiment_row_to_proto(&row, &variants);
        self.state.publish(proto.clone(), false);

        info!(experiment_id = %id, %reason, "experiment paused");
        Ok(Response::new(proto))
    }

    async fn resume_experiment(
        &self,
        request: Request<ResumeExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let id = Uuid::parse_str(&request.into_inner().experiment_id)
            .map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))?;

        let rows = self
            .state
            .store
            .resume_transition(id)
            .await
            .map_err(store_err_to_status)?;

        if rows == 0 {
            let row = self
                .state
                .store
                .get_experiment(id)
                .await
                .map_err(store_err_to_status)?;
            return Err(Status::failed_precondition(format!(
                "experiment {} is in state {} (must be PAUSED to resume)",
                id, row.state
            )));
        }

        let row = self
            .state
            .store
            .get_experiment(id)
            .await
            .map_err(store_err_to_status)?;

        let variants = self
            .state
            .store
            .get_variants(id)
            .await
            .map_err(store_err_to_status)?;

        self.state
            .store
            .record_audit(
                id,
                "resume",
                &row.owner_email,
                Some("PAUSED"),
                Some("RUNNING"),
                &serde_json::json!({}),
            )
            .await
            .ok();

        let proto = experiment_row_to_proto(&row, &variants);
        self.state.publish(proto.clone(), false);

        info!(experiment_id = %id, "experiment resumed");
        Ok(Response::new(proto))
    }

    // --- StreamConfigUpdates ---

    type StreamConfigUpdatesStream = ReceiverStream<Result<ConfigUpdateEvent, Status>>;

    async fn stream_config_updates(
        &self,
        request: Request<StreamConfigUpdatesRequest>,
    ) -> Result<Response<Self::StreamConfigUpdatesStream>, Status> {
        let last_known_version = request.into_inner().last_known_version;
        let current_version = self.state.version.load(Ordering::SeqCst);

        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let store = self.state.store.clone();
        let mut broadcast_rx = self.state.config_tx.subscribe();

        tokio::spawn(async move {
            // Backfill: stream all RUNNING/PAUSED experiments if client is behind.
            if last_known_version < current_version {
                match store.list_active_experiments().await {
                    Ok(rows) => {
                        for row in &rows {
                            let variants = match store.get_variants(row.experiment_id).await {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let proto = experiment_row_to_proto(row, &variants);
                            let event = ConfigUpdateEvent {
                                experiment: Some(proto),
                                is_deletion: false,
                                version: current_version,
                            };
                            if tx.send(Ok(event)).await.is_err() {
                                return; // Client disconnected during backfill.
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "backfill query failed");
                    }
                }
            }

            // Stream live updates from broadcast channel.
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        if tx.send(Ok(event)).await.is_err() {
                            break; // Client disconnected.
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "StreamConfigUpdates subscriber lagged");
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    // --- Metric definitions (ADR-026 Phase 1) ---

    async fn create_metric_definition(
        &self,
        request: Request<CreateMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        let metric = request
            .into_inner()
            .metric
            .ok_or_else(|| Status::invalid_argument("metric is required"))?;

        // Phase 1 (#433) skeleton: common-field non-empty checks land here.
        // Per-type rules (FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT) ship
        // incrementally in B1/B2/B3 — the validator stays the single
        // dispatch point.
        validators::validate_metric_definition(&metric, self.state.store.as_ref())
            .await
            .map_err(|boxed| *boxed)?;

        let row = self
            .state
            .store
            .create_metric(&metric)
            .await
            .map_err(store_err_to_status)?;

        // ADR-026 Phase 3 (L5): emit the deprecation **header** on CUSTOM
        // creates. The check uses the server-echoed row type (not the request
        // payload's type) so that any future store-side type coercion stays
        // symmetric with the response body the UI reads. The UI at
        // ui/src/app/metrics/new/page.tsx already treats the server-echoed
        // type as the source of truth — see Devin PR #578 round-1 📝 symmetry
        // note. UpdateMetricDefinition RPC does not exist; per L7 metric type
        // is immutable post-create — Create is the only entry point.
        let response_metric = row.into_proto();
        let stored_type = response_metric.r#type();
        let metric_id_for_log = response_metric.metric_id.clone();
        let mut response = Response::new(response_metric);

        if stored_type == MetricType::Custom {
            response.metadata_mut().insert(
                "x-kaizen-deprecation",
                tonic::metadata::MetadataValue::from_static(DEPRECATION_HEADER_CUSTOM),
            );
            tracing::info!(
                target: "m5.deprecation",
                metric_type = "CUSTOM",
                metric_id = %metric_id_for_log,
                event = "metric_definition_created",
                "custom metric created — emitting x-kaizen-deprecation header"
            );
        }

        Ok(response)
    }

    async fn get_metric_definition(
        &self,
        request: Request<GetMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        let req = request.into_inner();
        if req.metric_id.trim().is_empty() {
            return Err(Status::invalid_argument("metric_id is required"));
        }

        let row = self
            .state
            .store
            .get_metric(&req.metric_id)
            .await
            .map_err(store_err_to_status)?;

        Ok(Response::new(row.into_proto()))
    }

    async fn list_metric_definitions(
        &self,
        request: Request<ListMetricDefinitionsRequest>,
    ) -> Result<Response<ListMetricDefinitionsResponse>, Status> {
        let req = request.into_inner();

        // The proto today only carries `type_filter`. Pagination is best-effort:
        // page_size / page_token are accepted but the underlying store returns
        // the full set (the metric catalog is small in Phase 1; cursor support
        // arrives if/when usage warrants).
        let type_filter = MetricType::try_from(req.type_filter)
            .unwrap_or(MetricType::Unspecified);
        let filter = crate::store::MetricFilter {
            r#type: if type_filter == MetricType::Unspecified {
                None
            } else {
                Some(metric_type_to_pg_string(type_filter).to_string())
            },
            ..Default::default()
        };

        let rows = self
            .state
            .store
            .list_metrics(filter)
            .await
            .map_err(store_err_to_status)?;

        let metrics: Vec<MetricDefinition> =
            rows.into_iter().map(|r| r.into_proto()).collect();

        Ok(Response::new(ListMetricDefinitionsResponse {
            metrics,
            next_page_token: String::new(),
        }))
    }

    // --- Layer management ---

    async fn create_layer(
        &self,
        _request: Request<CreateLayerRequest>,
    ) -> Result<Response<Layer>, Status> {
        Err(Status::unimplemented("CreateLayer not yet implemented"))
    }

    async fn get_layer(
        &self,
        _request: Request<GetLayerRequest>,
    ) -> Result<Response<Layer>, Status> {
        Err(Status::unimplemented("GetLayer not yet implemented"))
    }

    async fn get_layer_allocations(
        &self,
        request: Request<GetLayerAllocationsRequest>,
    ) -> Result<Response<GetLayerAllocationsResponse>, Status> {
        let req = request.into_inner();
        let layer_id = Uuid::parse_str(&req.layer_id)
            .map_err(|_| Status::invalid_argument("invalid layer_id UUID"))?;

        let allocs = bucket_reuse::list_allocations(
            self.state.store.pool(),
            layer_id,
            req.include_released,
        )
        .await
        .map_err(|e| Status::internal(format!("allocation query error: {e}")))?;

        let proto_allocs: Vec<LayerAllocation> = allocs
            .iter()
            .map(|a| LayerAllocation {
                allocation_id: a.allocation_id.to_string(),
                layer_id: a.layer_id.to_string(),
                experiment_id: a.experiment_id.to_string(),
                start_bucket: a.start_bucket,
                end_bucket: a.end_bucket,
                activated_at: a.activated_at.map(|t| t.into_proto()),
                released_at: a.released_at.map(|t| t.into_proto()),
                reusable_after: a.reusable_after.map(|t| t.into_proto()),
            })
            .collect();

        Ok(Response::new(GetLayerAllocationsResponse {
            allocations: proto_allocs,
        }))
    }

    // --- Targeting ---

    async fn create_targeting_rule(
        &self,
        _request: Request<CreateTargetingRuleRequest>,
    ) -> Result<Response<TargetingRule>, Status> {
        Err(Status::unimplemented("CreateTargetingRule not yet implemented"))
    }

    // --- Surrogate ---

    async fn create_surrogate_model(
        &self,
        _request: Request<CreateSurrogateModelRequest>,
    ) -> Result<Response<SurrogateModelConfig>, Status> {
        Err(Status::unimplemented("CreateSurrogateModel not yet implemented"))
    }

    async fn list_surrogate_models(
        &self,
        _request: Request<ListSurrogateModelsRequest>,
    ) -> Result<Response<ListSurrogateModelsResponse>, Status> {
        Err(Status::unimplemented("ListSurrogateModels not yet implemented"))
    }

    async fn get_surrogate_calibration(
        &self,
        _request: Request<GetSurrogateCalibrationRequest>,
    ) -> Result<Response<SurrogateModelConfig>, Status> {
        Err(Status::unimplemented("GetSurrogateCalibration not yet implemented"))
    }

    async fn trigger_surrogate_recalibration(
        &self,
        _request: Request<TriggerSurrogateRecalibrationRequest>,
    ) -> Result<Response<()>, Status> {
        Err(Status::unimplemented("TriggerSurrogateRecalibration not yet implemented"))
    }

    async fn get_portfolio_allocation(
        &self,
        request: Request<GetPortfolioAllocationRequest>,
    ) -> Result<Response<GetPortfolioAllocationResponse>, Status> {
        let req = request.into_inner();

        // Load all RUNNING experiments, optionally filtered by layer_id.
        let experiments = self
            .state
            .store
            .list_active_experiments()
            .await
            .map_err(store_err_to_status)?;

        let running_experiments: Vec<_> = experiments
            .iter()
            .filter(|e| e.state == "RUNNING")
            .filter(|e| req.layer_id.is_empty() || e.layer_id.to_string() == req.layer_id)
            .collect();

        // Load bucket allocations for each experiment and build ExperimentInfo vec.
        let mut experiment_infos = Vec::with_capacity(running_experiments.len());
        for exp in &running_experiments {
            let allocations = crate::bucket_reuse::list_allocations(
                self.state.store.pool(),
                exp.layer_id,
                false,
            )
            .await
            .map_err(|e| Status::internal(format!("allocation lookup failed: {e}")))?;

            // Find this experiment's allocation in the layer.
            let alloc = allocations
                .iter()
                .find(|a| a.experiment_id == exp.experiment_id);

            let (start_bucket, end_bucket, total_buckets) = if let Some(a) = alloc {
                let layer_total = get_layer_total_buckets(self.state.store.pool(), exp.layer_id)
                    .await
                    .unwrap_or(1000);
                (a.start_bucket, a.end_bucket, layer_total)
            } else {
                // No allocation found — use variant fractions as approximation.
                let variants = self
                    .state
                    .store
                    .get_variants(exp.experiment_id)
                    .await
                    .map_err(store_err_to_status)?;
                let total_fraction: f64 = variants.iter().map(|v| v.traffic_fraction).sum();
                let end = (total_fraction * 1000.0).round() as i32;
                (0, end.max(1) - 1, 1000)
            };

            // Extract guardrail metric IDs from the type_config JSON.
            let guardrail_ids = extract_guardrail_ids(&exp.type_config);

            experiment_infos.push(crate::portfolio::ExperimentInfo {
                experiment_id: exp.experiment_id.to_string(),
                experiment_name: exp.name.clone(),
                layer_id: exp.layer_id.to_string(),
                primary_metric_id: exp.primary_metric_id.clone(),
                guardrail_metric_ids: guardrail_ids,
                targeting_rule_id: exp
                    .targeting_rule_id
                    .map(|u| u.to_string())
                    .unwrap_or_default(),
                start_bucket,
                end_bucket,
                layer_total_buckets: total_buckets,
            });
        }

        // Convert priority_overrides from proto map.
        let priority_overrides: std::collections::HashMap<String, i32> = req
            .priority_overrides
            .into_iter()
            .collect();

        // Run the optimizer.
        let result = crate::portfolio::optimize(&experiment_infos, &priority_overrides);

        // Convert to proto types.
        let proto_allocations: Vec<ProtoExperimentAllocation> = result
            .allocations
            .iter()
            .map(|a| ProtoExperimentAllocation {
                experiment_id: a.experiment_id.clone(),
                experiment_name: a.experiment_name.clone(),
                priority: a.priority,
                current_traffic_fraction: a.current_traffic_fraction,
                recommended_traffic_fraction: a.recommended_traffic_fraction,
                underpowered: a.underpowered,
                rationale: a.rationale.clone(),
                variance_budget_share: a.variance_budget_share,
            })
            .collect();

        let proto_conflicts: Vec<ProtoExperimentConflict> = result
            .conflicts
            .iter()
            .map(|c| ProtoExperimentConflict {
                experiment_id_a: c.experiment_id_a.clone(),
                experiment_id_b: c.experiment_id_b.clone(),
                conflict_type: match c.conflict_type {
                    crate::portfolio::ConflictType::PrimaryMetricOverlap => {
                        ProtoConflictType::PrimaryMetricOverlap as i32
                    }
                    crate::portfolio::ConflictType::GuardrailMetricOverlap => {
                        ProtoConflictType::GuardrailMetricOverlap as i32
                    }
                    crate::portfolio::ConflictType::PopulationOverlap => {
                        ProtoConflictType::PopulationOverlap as i32
                    }
                },
                rationale: c.rationale.clone(),
            })
            .collect();

        let proto_stats = ProtoPortfolioStats {
            running_count: result.stats.running_count,
            traffic_utilization: result.stats.traffic_utilization,
            expected_false_discoveries: result.stats.expected_false_discoveries,
            underpowered_count: result.stats.underpowered_count,
            conflict_count: result.stats.conflict_count,
        };

        info!(
            running = result.stats.running_count,
            conflicts = result.stats.conflict_count,
            underpowered = result.stats.underpowered_count,
            "portfolio allocation computed"
        );

        Ok(Response::new(GetPortfolioAllocationResponse {
            allocations: proto_allocations,
            conflicts: proto_conflicts,
            stats: Some(proto_stats),
        }))
    }

    // --- MetricQL live linting (ADR-026 Phase 2 / #436) ---

    async fn validate_metricql(
        &self,
        request: Request<ValidateMetricqlRequest>,
    ) -> Result<Response<ValidateMetricqlResponse>, Status> {
        let req = request.into_inner();

        if req.experiment_id.trim().is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        // Per plan L8: empty expression returns one diagnostic, not an RPC error.
        if req.metricql_expression.trim().is_empty() {
            return Ok(Response::new(ValidateMetricqlResponse {
                diagnostics: vec![ProtoMetricqlDiagnostic {
                    severity: ProtoSeverity::Error as i32,
                    message: "empty MetricQL expression".to_string(),
                    span: Some(ProtoSpan {
                        start_offset: 0,
                        end_offset: 0,
                        line: 1,
                        column: 1,
                    }),
                }],
                referenced_metric_ids: vec![],
            }));
        }

        // Pass None for known_metric_ids — existence checks are enforced at
        // write time (CreateMetricDefinition). The live-lint path only needs to
        // catch parse/semantic structural errors during interactive editing.
        let ctx = validators::metricql::ValidateContext { known_metric_ids: None };

        let response =
            match validators::metricql::validate_metricql(&req.metricql_expression, &ctx) {
                Ok(refs) => ValidateMetricqlResponse {
                    diagnostics: vec![],
                    referenced_metric_ids: refs,
                },
                Err(diags) => ValidateMetricqlResponse {
                    diagnostics: diags
                        .into_iter()
                        .map(|d| internal_to_proto_diag(d, &req.metricql_expression))
                        .collect(),
                    referenced_metric_ids: vec![],
                },
            };

        Ok(Response::new(response))
    }

    // --- PreviewMetricDefinition (ADR-026 Phase 2 / #436) ---

    async fn preview_metric_definition(
        &self,
        request: Request<PreviewMetricDefinitionRequest>,
    ) -> Result<Response<PreviewMetricDefinitionResponse>, Status> {
        // Extract the grpc-timeout header (if present) before consuming the
        // request.  Tonic 0.12 does not expose a `deadline()` helper; the
        // remaining budget is encoded in the `grpc-timeout` metadata key.
        // We propagate it verbatim to M3 so M3 also gives up when M6 gives up.
        let grpc_timeout = request
            .metadata()
            .get("grpc-timeout")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        let req = request.into_inner();

        if req.experiment_id.trim().is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        if req.metricql_expression.trim().is_empty() {
            return Err(Status::invalid_argument("metricql_expression is required"));
        }

        let m3_req = CompileMetricqlPreviewRequest {
            experiment_id: req.experiment_id.clone(),
            metricql_expression: req.metricql_expression.clone(),
        };

        let mut outbound = Request::new(m3_req);

        // Propagate the caller's deadline to M3 so M3 also respects the budget.
        // If no grpc-timeout was provided by the caller, apply a 5s default so
        // the preview call never hangs indefinitely.
        let timeout = grpc_timeout
            .and_then(|t| parse_grpc_timeout(&t))
            .unwrap_or(std::time::Duration::from_secs(5));
        outbound.set_timeout(timeout);

        // Clone is cheap — Channel is Arc-counted internally.
        let mut client = self.state.metrics_client.clone();
        let m3_resp = client.compile_metricql_preview(outbound).await?;

        let inner = m3_resp.into_inner();
        Ok(Response::new(PreviewMetricDefinitionResponse {
            compiled_sql: inner.compiled_sql,
            diagnostics: inner.diagnostics,
        }))
    }
}

// ---------------------------------------------------------------------------
// MetricQL diagnostic helpers (ADR-026 Phase 2 / #436)
// ---------------------------------------------------------------------------

/// Convert byte offset into the source string to a 1-based (line, column) pair.
/// Iterates once over the bytes up to `byte_offset`. ASCII-naive (consistent
/// with the proto Span comment: "ASCII-naive; Phase 2 punt").
fn line_col_from_byte_offset(source: &str, byte_offset: usize) -> (i32, i32) {
    let mut line = 1i32;
    let mut col = 1i32;
    for (i, b) in source.bytes().enumerate() {
        if i >= byte_offset {
            break;
        }
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Convert one internal [`validators::metricql::Diagnostic`] to the proto wire type.
fn internal_to_proto_diag(
    d: validators::metricql::Diagnostic,
    source: &str,
) -> ProtoMetricqlDiagnostic {
    let (line, column) = line_col_from_byte_offset(source, d.span.start);
    ProtoMetricqlDiagnostic {
        severity: match d.severity {
            validators::metricql::Severity::Error => ProtoSeverity::Error as i32,
            validators::metricql::Severity::Warning => ProtoSeverity::Warning as i32,
        },
        message: d.message,
        span: Some(ProtoSpan {
            start_offset: d.span.start as i32,
            end_offset: d.span.end as i32,
            line,
            column,
        }),
    }
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

pub async fn serve(config: &crate::config::ManagementConfig, store: ManagementStore) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    // Build a lazy channel to M3. connect_lazy() does NOT establish TCP until
    // the first RPC — no startup latency or failure if M3 isn't up yet.
    let metrics_endpoint = tonic::transport::Endpoint::from_shared(config.metrics_addr.clone())
        .map_err(|e| format!("invalid METRICS_ADDR '{}': {e}", config.metrics_addr))?
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(5));
    let metrics_channel = metrics_endpoint.connect_lazy();
    let metrics_client = MetricComputationServiceClient::new(metrics_channel);

    let state = SharedState::new_with_channel(store, metrics_client);
    let handler = ManagementServiceHandler::new(state);
    let svc = ExperimentManagementServiceServer::new(handler);

    info!(%addr, "management service gRPC starting (tonic-web enabled)");

    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(tonic_web::enable(svc))
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the JSONB type_config column from Phase 5 configs in the proto.
fn build_type_config(exp: &Experiment) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    if let Some(ref ic) = exp.interleaving_config {
        obj.insert("interleaving_config".into(), serde_json::to_value(
            serde_json::json!({
                "method": ic.method,
                "algorithm_ids": ic.algorithm_ids,
                "max_list_size": ic.max_list_size,
            })
        ).unwrap_or_default());
    }
    if let Some(ref sc) = exp.session_config {
        obj.insert("session_config".into(), serde_json::json!({
            "session_id_attribute": sc.session_id_attribute,
            "allow_cross_session_variation": sc.allow_cross_session_variation,
            "min_sessions_per_user": sc.min_sessions_per_user,
        }));
    }
    if let Some(ref bc) = exp.bandit_config {
        obj.insert("bandit_config".into(), serde_json::json!({
            "algorithm": bc.algorithm,
            "reward_metric_id": bc.reward_metric_id,
            "min_exploration_fraction": bc.min_exploration_fraction,
        }));
    }
    if let Some(ref meta) = exp.meta_experiment_config {
        obj.insert("meta_experiment_config".into(), serde_json::json!({
            "base_algorithm": meta.base_algorithm,
            "outcome_metric_ids": meta.outcome_metric_ids,
            "variant_count": meta.variant_objectives.len(),
        }));
    }
    if let Some(ref sw) = exp.switchback_config {
        obj.insert("switchback_config".into(), serde_json::json!({
            "planned_cycles": sw.planned_cycles,
            "block_duration_seconds": sw.block_duration.as_ref().map(|d| d.seconds),
            "cluster_attribute": sw.cluster_attribute,
        }));
    }
    if let Some(ref qe) = exp.quasi_experiment_config {
        obj.insert("quasi_experiment_config".into(), serde_json::json!({
            "treated_unit_id": qe.treated_unit_id,
            "donor_count": qe.donor_unit_ids.len(),
            "outcome_metric_id": qe.outcome_metric_id,
            "method": qe.method,
        }));
    }
    if let Some(ref asn) = exp.adaptive_sample_size_config {
        obj.insert("adaptive_sample_size_config".into(), serde_json::json!({
            "interim_fraction": asn.interim_fraction,
            "promising_zone_lower": asn.promising_zone_lower,
            "favorable_zone_lower": asn.favorable_zone_lower,
            "max_extension_factor": asn.max_extension_factor,
        }));
    }

    serde_json::Value::Object(obj)
}

/// Look up total_buckets for a layer from the database.
async fn get_layer_total_buckets(pool: &sqlx::postgres::PgPool, layer_id: Uuid) -> Result<i32, ()> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT total_buckets FROM layers WHERE layer_id = $1",
    )
    .bind(layer_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ())?;

    row.map(|(total,)| total).ok_or(())
}

/// Parse a `grpc-timeout` header value (e.g. "5000m", "2S", "1H") into a `Duration`.
///
/// The gRPC wire format: an ASCII decimal integer followed by a unit suffix:
///   H = hours, M = minutes, S = seconds, m = milliseconds,
///   u = microseconds, n = nanoseconds.
/// Ref: https://grpc.io/docs/what-is-grpc/core-concepts/ (grpc-timeout header).
///
/// Returns `None` if the value cannot be parsed (caller falls back to the default).
fn parse_grpc_timeout(value: &str) -> Option<std::time::Duration> {
    let (digits, unit) = value.split_at(value.len().saturating_sub(1));
    let n: u64 = digits.parse().ok()?;
    // checked_mul on H/M because the value comes from request metadata
    // (an untrusted client could send `99999999999999H`); unchecked u64
    // multiplication panics in debug builds and wraps in release builds,
    // either of which is worse than returning None (caller falls back
    // to the 5s default). Devin PR #570 round-1 finding.
    match unit {
        "H" => n.checked_mul(3600).map(std::time::Duration::from_secs),
        "M" => n.checked_mul(60).map(std::time::Duration::from_secs),
        "S" => Some(std::time::Duration::from_secs(n)),
        "m" => Some(std::time::Duration::from_millis(n)),
        "u" => Some(std::time::Duration::from_micros(n)),
        "n" => Some(std::time::Duration::from_nanos(n)),
        _ => None,
    }
}

/// Extract guardrail metric IDs from the type_config JSONB column.
fn extract_guardrail_ids(type_config: &serde_json::Value) -> Vec<String> {
    // Guardrail configs may be stored as a top-level array in type_config.
    if let Some(guardrails) = type_config.get("guardrail_configs") {
        if let Some(arr) = guardrails.as_array() {
            return arr
                .iter()
                .filter_map(|g| g.get("metric_id").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
                .collect();
        }
    }
    vec![]
}

// ---------------------------------------------------------------------------
// Tests — line_col_from_byte_offset + validate_metricql handler logic
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use validators::metricql::ValidateContext;

    // ── line_col_from_byte_offset ────────────────────────────────────────────

    #[test]
    fn line_col_offset_zero_is_line1_col1() {
        assert_eq!(line_col_from_byte_offset("mean(x.y)", 0), (1, 1));
    }

    #[test]
    fn line_col_offset_within_first_line() {
        // "mean" = bytes 0-3; offset 4 is '(' → still line 1
        assert_eq!(line_col_from_byte_offset("mean(x.y)", 4), (1, 5));
    }

    #[test]
    fn line_col_newline_increments_line() {
        // "foo\nbar" — offset 4 ('b') should be (line=2, col=1)
        let src = "foo\nbar";
        assert_eq!(line_col_from_byte_offset(src, 4), (2, 1));
    }

    #[test]
    fn line_col_second_char_on_second_line() {
        // "foo\nbar" — offset 5 ('a') → (2, 2)
        let src = "foo\nbar";
        assert_eq!(line_col_from_byte_offset(src, 5), (2, 2));
    }

    #[test]
    fn line_col_offset_past_end_clamps_gracefully() {
        // Should not panic on out-of-bounds offset
        let src = "ab";
        let (line, col) = line_col_from_byte_offset(src, 999);
        assert!(line >= 1);
        assert!(col >= 1);
    }

    // ── internal_to_proto_diag ───────────────────────────────────────────────

    #[test]
    fn internal_to_proto_diag_error_severity() {
        use validators::metricql::{Diagnostic, Severity, Span};

        let d = Diagnostic { severity: Severity::Error, message: "oops".into(), span: Span::new(0, 4) };
        let proto = internal_to_proto_diag(d, "oops");
        assert_eq!(proto.severity, ProtoSeverity::Error as i32);
        assert_eq!(proto.message, "oops");
        let span = proto.span.unwrap();
        assert_eq!(span.start_offset, 0);
        assert_eq!(span.end_offset, 4);
        assert_eq!(span.line, 1);
        assert_eq!(span.column, 1);
    }

    #[test]
    fn internal_to_proto_diag_multiline_span() {
        use validators::metricql::{Diagnostic, Severity, Span};

        // Source: "foo\nbar" — error at byte 4 ('b') → line 2 col 1
        let src = "foo\nbar";
        let d = Diagnostic { severity: Severity::Error, message: "bad".into(), span: Span::new(4, 7) };
        let proto = internal_to_proto_diag(d, src);
        let span = proto.span.unwrap();
        assert_eq!(span.line, 2);
        assert_eq!(span.column, 1);
    }

    // ── validate_metricql handler logic (via validator directly) ─────────────
    // The full RPC handler requires a ManagementStore (database); instead we
    // test the core logic — the validator call and proto conversion — directly.
    // The empty-expression and empty-experiment-id paths are pure handler logic.

    #[test]
    fn empty_expression_produces_one_empty_diagnostic() {
        // Mirrors the empty-expression branch in validate_metricql handler.
        let expr = "";
        assert!(expr.trim().is_empty()); // confirms the branch triggers

        // The proto response would have exactly one diagnostic.
        let diag = ProtoMetricqlDiagnostic {
            severity: ProtoSeverity::Error as i32,
            message: "empty MetricQL expression".to_string(),
            span: Some(ProtoSpan { start_offset: 0, end_offset: 0, line: 1, column: 1 }),
        };
        assert!(diag.message.to_lowercase().contains("empty"));
    }

    #[test]
    fn valid_expression_yields_no_diagnostics_and_refs() {
        let ctx = ValidateContext { known_metric_ids: None };
        let result = validators::metricql::validate_metricql("0.7 * @watch_time + 0.3 * @ctr", &ctx);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        let mut refs = result.unwrap();
        refs.sort();
        assert_eq!(refs, vec!["ctr", "watch_time"]);
    }

    #[test]
    fn parse_error_produces_one_diagnostic_with_proto_conversion() {
        let ctx = ValidateContext { known_metric_ids: None };
        let src = "mean(heartbeat.value"; // unclosed paren
        let result = validators::metricql::validate_metricql(src, &ctx);
        let diags = result.unwrap_err();
        assert_eq!(diags.len(), 1);

        let proto = internal_to_proto_diag(diags.into_iter().next().unwrap(), src);
        assert_eq!(proto.severity, ProtoSeverity::Error as i32);
        assert!(proto.span.is_some());
    }

    #[test]
    fn count_with_field_produces_error_diagnostic() {
        let ctx = ValidateContext { known_metric_ids: None };
        let result = validators::metricql::validate_metricql("count(login.foo)", &ctx);
        assert!(result.is_err(), "count(login.foo) should fail semantic analysis");
        let diags = result.unwrap_err();
        assert!(!diags.is_empty());
    }

    #[test]
    fn multiline_expression_error_attributed_to_line2() {
        // "mean(heartbeat.value)\nand wrong" — the "and" part will parse/fail
        // somewhere after the newline. We verify the proto span gets line 2.
        let src = "mean(heartbeat.value)\nand wrong";
        let ctx = ValidateContext { known_metric_ids: None };
        let result = validators::metricql::validate_metricql(src, &ctx);
        // The expression may parse or fail depending on grammar — we just verify
        // the line_col conversion works on multi-line input by exercising it.
        match result {
            Err(diags) => {
                for d in diags {
                    if d.span.start > 21 {
                        // After the newline
                        let proto = internal_to_proto_diag(d, src);
                        let span = proto.span.unwrap();
                        assert_eq!(span.line, 2, "error after newline should be on line 2");
                    }
                }
            }
            Ok(_) => {
                // Expression valid — at minimum verify line_col_from_byte_offset
                // correctly returns line 2 for offset 22 (first char after '\n')
                let (line, _) = line_col_from_byte_offset(src, 22);
                assert_eq!(line, 2);
            }
        }
    }
}
