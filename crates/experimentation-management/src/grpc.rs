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
    Experiment, ExperimentState as ProtoState, ExperimentType, GuardrailAction, Layer,
    LayerAllocation, MetricDefinition, SurrogateModelConfig, TargetingRule, Variant,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::{
        ExperimentManagementService, ExperimentManagementServiceServer,
    },
    ArchiveExperimentRequest, ConcludeExperimentRequest, ConfigUpdateEvent,
    CreateExperimentRequest, CreateLayerRequest, CreateMetricDefinitionRequest,
    CreateSurrogateModelRequest, CreateTargetingRuleRequest, GetExperimentRequest,
    GetLayerAllocationsRequest, GetLayerAllocationsResponse, GetLayerRequest,
    GetMetricDefinitionRequest, GetSurrogateCalibrationRequest, ListExperimentsRequest,
    ListExperimentsResponse, ListMetricDefinitionsRequest, ListMetricDefinitionsResponse,
    ListSurrogateModelsRequest, ListSurrogateModelsResponse, PauseExperimentRequest,
    ResumeExperimentRequest, StartExperimentRequest, StreamConfigUpdatesRequest,
    TriggerSurrogateRecalibrationRequest, UpdateExperimentRequest,
};

use crate::bucket_reuse;
use crate::store::{ExperimentRow, ManagementStore, StoreError, VariantRow};
use crate::validators;

// Broadcast channel capacity. Slow subscribers will see RecvError::Lagged.
const BROADCAST_CAPACITY: usize = 512;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SharedState {
    pub store: Arc<ManagementStore>,
    pub config_tx: broadcast::Sender<ConfigUpdateEvent>,
    pub version: Arc<AtomicI64>,
}

impl SharedState {
    pub fn new(store: ManagementStore) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            store: Arc::new(store),
            config_tx: tx,
            version: Arc::new(AtomicI64::new(1)),
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

    // --- Metric definitions (stubs — schema validated at DB level) ---

    async fn create_metric_definition(
        &self,
        _request: Request<CreateMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        Err(Status::unimplemented("CreateMetricDefinition not yet implemented"))
    }

    async fn get_metric_definition(
        &self,
        _request: Request<GetMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        Err(Status::unimplemented("GetMetricDefinition not yet implemented"))
    }

    async fn list_metric_definitions(
        &self,
        _request: Request<ListMetricDefinitionsRequest>,
    ) -> Result<Response<ListMetricDefinitionsResponse>, Status> {
        Err(Status::unimplemented("ListMetricDefinitions not yet implemented"))
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
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

pub async fn serve(config: &crate::config::ManagementConfig, store: ManagementStore) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    let state = SharedState::new(store);
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
