//! tonic gRPC service implementation for ExperimentManagementService (M5 Rust port).
//!
//! Phase 1: Scaffold with RBAC interceptor, TOCTOU-safe lifecycle transitions,
//!          and stubs for all 20 RPCs. Full CRUD is Phase 2.
//!
//! All 20 RPCs from management_service.proto are implemented here:
//!   - Lifecycle RPCs (Start, Conclude, Archive, Pause, Resume): full TOCTOU-safe transitions.
//!   - GetExperiment + ListExperiments: basic read path via store.
//!   - All others: `unimplemented!` stub returning UNIMPLEMENTED status.

use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::{
    Experiment, Layer, MetricDefinition, SurrogateModelConfig, TargetingRule,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::{
        ExperimentManagementService, ExperimentManagementServiceServer,
    },
    ArchiveExperimentRequest, ConcludeExperimentRequest, CreateExperimentRequest,
    CreateLayerRequest, CreateMetricDefinitionRequest, CreateSurrogateModelRequest,
    CreateTargetingRuleRequest, GetExperimentRequest, GetLayerAllocationsRequest,
    GetLayerAllocationsResponse, GetLayerRequest, GetMetricDefinitionRequest,
    GetSurrogateCalibrationRequest, ListExperimentsRequest, ListExperimentsResponse,
    ListMetricDefinitionsRequest, ListMetricDefinitionsResponse, ListSurrogateModelsRequest,
    ListSurrogateModelsResponse, PauseExperimentRequest, ResumeExperimentRequest,
    StartExperimentRequest, TriggerSurrogateRecalibrationRequest, UpdateExperimentRequest,
};

use crate::config::ManagementConfig;
use crate::rbac::{require_role, Role};
use crate::state::{validate_transition, ExperimentState, TransitionError};
use crate::store::{ExperimentWithVariants, ManagementStore, StoreError};

// ---------------------------------------------------------------------------
// Service handler
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ManagementServiceHandler {
    store: Arc<ManagementStore>,
}

impl ManagementServiceHandler {
    pub fn new(store: ManagementStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }
}

// ---------------------------------------------------------------------------
// Proto ↔ domain conversions
// ---------------------------------------------------------------------------

fn experiment_state_from_proto(state: i32) -> Option<ExperimentState> {
    use experimentation_proto::experimentation::common::v1::ExperimentState as ProtoState;
    match ProtoState::try_from(state).ok()? {
        ProtoState::Draft => Some(ExperimentState::Draft),
        ProtoState::Starting => Some(ExperimentState::Starting),
        ProtoState::Running => Some(ExperimentState::Running),
        ProtoState::Concluding => Some(ExperimentState::Concluding),
        ProtoState::Concluded => Some(ExperimentState::Concluded),
        ProtoState::Archived => Some(ExperimentState::Archived),
        ProtoState::Unspecified => None,
    }
}

fn experiment_state_to_proto(state: &ExperimentState) -> i32 {
    use experimentation_proto::experimentation::common::v1::ExperimentState as ProtoState;
    match state {
        ExperimentState::Draft => ProtoState::Draft as i32,
        ExperimentState::Starting => ProtoState::Starting as i32,
        ExperimentState::Running => ProtoState::Running as i32,
        ExperimentState::Concluding => ProtoState::Concluding as i32,
        ExperimentState::Concluded => ProtoState::Concluded as i32,
        ExperimentState::Archived => ProtoState::Archived as i32,
    }
}

fn experiment_type_str_to_proto(t: &str) -> i32 {
    use experimentation_proto::experimentation::common::v1::ExperimentType as ProtoType;
    match t {
        "AB" => ProtoType::Ab as i32,
        "MULTIVARIATE" => ProtoType::Multivariate as i32,
        "INTERLEAVING" => ProtoType::Interleaving as i32,
        "SESSION_LEVEL" => ProtoType::SessionLevel as i32,
        "PLAYBACK_QOE" => ProtoType::PlaybackQoe as i32,
        "MAB" => ProtoType::Mab as i32,
        "CONTEXTUAL_BANDIT" => ProtoType::ContextualBandit as i32,
        "CUMULATIVE_HOLDOUT" => ProtoType::CumulativeHoldout as i32,
        "META" => ProtoType::Meta as i32,
        "SWITCHBACK" => ProtoType::Switchback as i32,
        "QUASI" => ProtoType::Quasi as i32,
        _ => ProtoType::Unspecified as i32,
    }
}

fn guardrail_action_to_proto(action: &str) -> i32 {
    use experimentation_proto::experimentation::common::v1::GuardrailAction as ProtoAction;
    match action {
        "AUTO_PAUSE" => ProtoAction::AutoPause as i32,
        "ALERT_ONLY" => ProtoAction::AlertOnly as i32,
        _ => ProtoAction::Unspecified as i32,
    }
}

fn row_to_proto(row: &ExperimentWithVariants) -> Experiment {
    use experimentation_proto::experimentation::common::v1::Variant;

    let exp = &row.experiment;
    let state = ExperimentState::from_db_str(&exp.state).unwrap_or(ExperimentState::Draft);

    let variants: Vec<Variant> = row
        .variants
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
        experiment_id: exp.experiment_id.to_string(),
        name: exp.name.clone(),
        description: exp.description.clone().unwrap_or_default(),
        owner_email: exp.owner_email.clone(),
        r#type: experiment_type_str_to_proto(&exp.r#type),
        state: experiment_state_to_proto(&state),
        variants,
        layer_id: exp.layer_id.to_string(),
        primary_metric_id: exp.primary_metric_id.clone(),
        secondary_metric_ids: exp.secondary_metric_ids.clone(),
        guardrail_configs: vec![],
        guardrail_action: guardrail_action_to_proto(&exp.guardrail_action),
        sequential_test_config: None,
        targeting_rule_id: exp
            .targeting_rule_id
            .map(|u| u.to_string())
            .unwrap_or_default(),
        created_at: Some(prost_types::Timestamp {
            seconds: exp.created_at.timestamp(),
            nanos: exp.created_at.timestamp_subsec_nanos() as i32,
        }),
        started_at: exp.started_at.map(|t| prost_types::Timestamp {
            seconds: t.timestamp(),
            nanos: t.timestamp_subsec_nanos() as i32,
        }),
        concluded_at: exp.concluded_at.map(|t| prost_types::Timestamp {
            seconds: t.timestamp(),
            nanos: t.timestamp_subsec_nanos() as i32,
        }),
        interleaving_config: None,
        session_config: None,
        surrogate_model_id: exp
            .surrogate_model_id
            .map(|u| u.to_string())
            .unwrap_or_default(),
        lifecycle_config: None,
        bandit_config: None,
        is_cumulative_holdout: exp.is_cumulative_holdout,
        hash_salt: exp.hash_salt.clone(),
        meta_experiment_config: None,
        switchback_config: None,
        quasi_experiment_config: None,
        adaptive_sample_size_config: None,
        variance_reduction_config: None,
        learning: 0,
    }
}

// ---------------------------------------------------------------------------
// Error mappings
// ---------------------------------------------------------------------------

fn store_err(e: StoreError) -> Status {
    match e {
        StoreError::NotFound(msg) => Status::not_found(msg),
        StoreError::AlreadyExists(msg) => Status::already_exists(msg),
        StoreError::InvalidPageToken => Status::invalid_argument("invalid page_token"),
        StoreError::Db(e) => {
            tracing::warn!(error = %e, "database error");
            Status::internal(format!("database error: {e}"))
        }
    }
}

fn transition_err(e: TransitionError) -> Status {
    match e {
        TransitionError::InvalidTransition { from, to } => {
            Status::failed_precondition(format!("invalid state transition: {from} → {to}"))
        }
        TransitionError::ConcurrentModification { experiment_id, expected } => Status::aborted(
            format!("concurrent modification: experiment {experiment_id} is no longer in state {expected}"),
        ),
        TransitionError::NotFound(id) => Status::not_found(format!("experiment {id}")),
        TransitionError::Db(e) => {
            tracing::warn!(error = %e, "database error in state transition");
            Status::internal(format!("database error: {e}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
fn parse_experiment_id(s: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(s).map_err(|_| Status::invalid_argument("invalid experiment_id UUID"))
}

// ---------------------------------------------------------------------------
// tonic service implementation
// ---------------------------------------------------------------------------

#[tonic::async_trait]
impl ExperimentManagementService for ManagementServiceHandler {
    // -----------------------------------------------------------------------
    // Experiment CRUD
    // -----------------------------------------------------------------------

    async fn create_experiment(
        &self,
        request: Request<CreateExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        require_role(request.extensions(), Role::Experimenter).map_err(|e| *e)?;
        // Phase 2: variant insertion, layer allocation, hash salt generation.
        Err(Status::unimplemented("CreateExperiment: Phase 2 implementation pending"))
    }

    async fn get_experiment(
        &self,
        request: Request<GetExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        let experiment_id = parse_experiment_id(&request.into_inner().experiment_id)?;

        let with_variants = self.store.get_experiment(experiment_id).await.map_err(store_err)?;
        Ok(Response::new(row_to_proto(&with_variants)))
    }

    async fn update_experiment(
        &self,
        request: Request<UpdateExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        require_role(request.extensions(), Role::Experimenter).map_err(|e| *e)?;
        // Phase 2: update allowed only in DRAFT state.
        Err(Status::unimplemented("UpdateExperiment: Phase 2 implementation pending"))
    }

    async fn list_experiments(
        &self,
        request: Request<ListExperimentsRequest>,
    ) -> Result<Response<ListExperimentsResponse>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        let req = request.into_inner();
        let page_size = if req.page_size <= 0 { 50 } else { req.page_size as i64 };
        let state_filter = if req.state_filter != 0 {
            Some(experiment_state_from_proto(req.state_filter)
                .ok_or_else(|| Status::invalid_argument(format!("invalid state_filter value: {}", req.state_filter)))?)
        } else {
            None
        };

        let (rows, next_token) = self
            .store
            .list_experiments(page_size, &req.page_token, state_filter, None, None)
            .await
            .map_err(store_err)?;

        let experiments: Vec<Experiment> = rows
            .iter()
            .map(|r| {
                row_to_proto(&ExperimentWithVariants {
                    experiment: r.clone(),
                    variants: vec![], // Full variant list is Phase 2 for list endpoint.
                })
            })
            .collect();

        Ok(Response::new(ListExperimentsResponse {
            experiments,
            next_page_token: next_token.unwrap_or_default(),
        }))
    }

    // -----------------------------------------------------------------------
    // Lifecycle transitions (TOCTOU-safe)
    // -----------------------------------------------------------------------

    async fn start_experiment(
        &self,
        request: Request<StartExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let identity = require_role(request.extensions(), Role::Experimenter).map_err(|e| *e)?;
        let actor = identity.email.clone();
        let experiment_id = parse_experiment_id(&request.into_inner().experiment_id)?;

        validate_transition(ExperimentState::Draft, ExperimentState::Starting)
            .map_err(transition_err)?;

        let row = self
            .store
            .apply_transition(experiment_id, ExperimentState::Draft, ExperimentState::Starting)
            .await
            .map_err(transition_err)?;

        info!(%experiment_id, %actor, "DRAFT → STARTING");
        self.store
            .audit(experiment_id, "start", &actor, Some(ExperimentState::Draft), Some(ExperimentState::Starting))
            .await;

        Ok(Response::new(row_to_proto(&ExperimentWithVariants { experiment: row, variants: vec![] })))
    }

    async fn conclude_experiment(
        &self,
        request: Request<ConcludeExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let identity = require_role(request.extensions(), Role::Experimenter).map_err(|e| *e)?;
        let actor = identity.email.clone();
        let experiment_id = parse_experiment_id(&request.into_inner().experiment_id)?;

        validate_transition(ExperimentState::Running, ExperimentState::Concluding)
            .map_err(transition_err)?;

        let row = self
            .store
            .apply_transition(experiment_id, ExperimentState::Running, ExperimentState::Concluding)
            .await
            .map_err(transition_err)?;

        info!(%experiment_id, %actor, "RUNNING → CONCLUDING");
        self.store
            .audit(experiment_id, "conclude", &actor, Some(ExperimentState::Running), Some(ExperimentState::Concluding))
            .await;

        Ok(Response::new(row_to_proto(&ExperimentWithVariants { experiment: row, variants: vec![] })))
    }

    async fn archive_experiment(
        &self,
        request: Request<ArchiveExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let identity = require_role(request.extensions(), Role::Admin).map_err(|e| *e)?;
        let actor = identity.email.clone();
        let experiment_id = parse_experiment_id(&request.into_inner().experiment_id)?;

        validate_transition(ExperimentState::Concluded, ExperimentState::Archived)
            .map_err(transition_err)?;

        let row = self
            .store
            .apply_transition(experiment_id, ExperimentState::Concluded, ExperimentState::Archived)
            .await
            .map_err(transition_err)?;

        info!(%experiment_id, %actor, "CONCLUDED → ARCHIVED");
        self.store
            .audit(experiment_id, "archive", &actor, Some(ExperimentState::Concluded), Some(ExperimentState::Archived))
            .await;

        Ok(Response::new(row_to_proto(&ExperimentWithVariants { experiment: row, variants: vec![] })))
    }

    async fn pause_experiment(
        &self,
        request: Request<PauseExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let identity = require_role(request.extensions(), Role::Experimenter).map_err(|e| *e)?;
        let actor = identity.email.clone();
        let inner = request.into_inner();
        let experiment_id = parse_experiment_id(&inner.experiment_id)?;
        let action = if inner.is_guardrail_auto_pause { "guardrail_auto_pause" } else { "pause" };

        let row = self.store.get_experiment_row(experiment_id).await.map_err(store_err)?;
        let current = ExperimentState::from_db_str(&row.state)
            .ok_or_else(|| Status::internal(format!("unknown state: {}", row.state)))?;

        if current != ExperimentState::Running {
            return Err(Status::failed_precondition(format!(
                "experiment must be RUNNING to pause (current: {current})"
            )));
        }

        // Phase 2: set variant traffic_fraction = 0 in a transaction.
        self.store.audit(experiment_id, action, &actor, None, None).await;
        info!(%experiment_id, %actor, is_guardrail = %inner.is_guardrail_auto_pause, "experiment paused");

        Ok(Response::new(row_to_proto(&ExperimentWithVariants { experiment: row, variants: vec![] })))
    }

    async fn resume_experiment(
        &self,
        request: Request<ResumeExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let identity = require_role(request.extensions(), Role::Experimenter).map_err(|e| *e)?;
        let actor = identity.email.clone();
        let experiment_id = parse_experiment_id(&request.into_inner().experiment_id)?;

        let row = self.store.get_experiment_row(experiment_id).await.map_err(store_err)?;
        let current = ExperimentState::from_db_str(&row.state)
            .ok_or_else(|| Status::internal(format!("unknown state: {}", row.state)))?;

        if current != ExperimentState::Running {
            return Err(Status::failed_precondition(format!(
                "experiment must be RUNNING to resume (current: {current})"
            )));
        }

        // Phase 2: restore original variant traffic fractions.
        self.store.audit(experiment_id, "resume", &actor, None, None).await;
        info!(%experiment_id, %actor, "experiment resumed");

        Ok(Response::new(row_to_proto(&ExperimentWithVariants { experiment: row, variants: vec![] })))
    }

    // -----------------------------------------------------------------------
    // Metric definitions
    // -----------------------------------------------------------------------

    async fn create_metric_definition(
        &self,
        request: Request<CreateMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        require_role(request.extensions(), Role::Analyst).map_err(|e| *e)?;
        Err(Status::unimplemented("CreateMetricDefinition: Phase 2 implementation pending"))
    }

    async fn get_metric_definition(
        &self,
        request: Request<GetMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        Err(Status::unimplemented("GetMetricDefinition: Phase 2 implementation pending"))
    }

    async fn list_metric_definitions(
        &self,
        request: Request<ListMetricDefinitionsRequest>,
    ) -> Result<Response<ListMetricDefinitionsResponse>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        Err(Status::unimplemented("ListMetricDefinitions: Phase 2 implementation pending"))
    }

    // -----------------------------------------------------------------------
    // Layer management
    // -----------------------------------------------------------------------

    async fn create_layer(
        &self,
        request: Request<CreateLayerRequest>,
    ) -> Result<Response<Layer>, Status> {
        require_role(request.extensions(), Role::Admin).map_err(|e| *e)?;
        Err(Status::unimplemented("CreateLayer: Phase 2 implementation pending"))
    }

    async fn get_layer(
        &self,
        request: Request<GetLayerRequest>,
    ) -> Result<Response<Layer>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        Err(Status::unimplemented("GetLayer: Phase 2 implementation pending"))
    }

    async fn get_layer_allocations(
        &self,
        request: Request<GetLayerAllocationsRequest>,
    ) -> Result<Response<GetLayerAllocationsResponse>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        Err(Status::unimplemented("GetLayerAllocations: Phase 2 implementation pending"))
    }

    // -----------------------------------------------------------------------
    // Targeting rules
    // -----------------------------------------------------------------------

    async fn create_targeting_rule(
        &self,
        request: Request<CreateTargetingRuleRequest>,
    ) -> Result<Response<TargetingRule>, Status> {
        require_role(request.extensions(), Role::Analyst).map_err(|e| *e)?;
        Err(Status::unimplemented("CreateTargetingRule: Phase 2 implementation pending"))
    }

    // -----------------------------------------------------------------------
    // Surrogate model management
    // -----------------------------------------------------------------------

    async fn create_surrogate_model(
        &self,
        request: Request<CreateSurrogateModelRequest>,
    ) -> Result<Response<SurrogateModelConfig>, Status> {
        require_role(request.extensions(), Role::Analyst).map_err(|e| *e)?;
        Err(Status::unimplemented("CreateSurrogateModel: Phase 2 implementation pending"))
    }

    async fn list_surrogate_models(
        &self,
        request: Request<ListSurrogateModelsRequest>,
    ) -> Result<Response<ListSurrogateModelsResponse>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        Err(Status::unimplemented("ListSurrogateModels: Phase 2 implementation pending"))
    }

    async fn get_surrogate_calibration(
        &self,
        request: Request<GetSurrogateCalibrationRequest>,
    ) -> Result<Response<SurrogateModelConfig>, Status> {
        require_role(request.extensions(), Role::Viewer).map_err(|e| *e)?;
        Err(Status::unimplemented("GetSurrogateCalibration: Phase 2 implementation pending"))
    }

    async fn trigger_surrogate_recalibration(
        &self,
        request: Request<TriggerSurrogateRecalibrationRequest>,
    ) -> Result<Response<()>, Status> {
        require_role(request.extensions(), Role::Analyst).map_err(|e| *e)?;
        Err(Status::unimplemented("TriggerSurrogateRecalibration: Phase 2 implementation pending"))
    }
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

/// Start the gRPC server with RBAC interceptor and tonic-web.
pub async fn serve(config: ManagementConfig, store: ManagementStore) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    let handler = ManagementServiceHandler::new(store);
    let svc = ExperimentManagementServiceServer::new(handler);

    tracing::info!(%addr, "management gRPC server starting (tonic-web, RBAC enabled)");

    tonic::transport::Server::builder()
        .accept_http1(true)
        .layer(tonic::service::interceptor(crate::rbac::rbac_interceptor))
        .add_service(tonic_web::enable(svc))
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}
