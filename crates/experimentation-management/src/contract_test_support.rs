//! In-memory store and handler for ADR-025 Phase 4 contract tests.
//!
//! The production handler (in `grpc.rs`) uses `ManagementStore` (sqlx + PostgreSQL).
//! Contract tests need a fast, DB-free implementation that verifies wire-format
//! compatibility. This module provides that in-memory implementation.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, Experiment, ExperimentState,
    ExperimentType, Layer, MetricDefinition, MetricType, TargetingRule,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    ArchiveExperimentRequest, ConcludeExperimentRequest, ConfigUpdateEvent,
    CreateExperimentRequest, CreateLayerRequest, CreateMetricDefinitionRequest,
    CreateSurrogateModelRequest, CreateTargetingRuleRequest, GetExperimentRequest,
    GetLayerAllocationsRequest, GetLayerAllocationsResponse, GetLayerRequest,
    GetMetricDefinitionRequest, GetSurrogateCalibrationRequest, ListExperimentsRequest,
    ListExperimentsResponse, ListMetricDefinitionsRequest, ListMetricDefinitionsResponse,
    ListSurrogateModelsRequest, ListSurrogateModelsResponse, PauseExperimentRequest,
    ResumeExperimentRequest, StartExperimentRequest, StreamConfigUpdatesRequest,
    GetPortfolioAllocationRequest, GetPortfolioAllocationResponse,
    TriggerSurrogateRecalibrationRequest, UpdateExperimentRequest,
    ValidateMetricqlRequest, ValidateMetricqlResponse,
};

// ---------------------------------------------------------------------------
// In-memory store
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ExperimentStore {
    experiments: Arc<RwLock<HashMap<String, Experiment>>>,
    layers: Arc<RwLock<HashMap<String, Layer>>>,
    version: Arc<RwLock<i64>>,
}

impl ExperimentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, exp: Experiment) -> Experiment {
        let mut map = self.experiments.write().unwrap();
        let mut ver = self.version.write().unwrap();
        *ver += 1;
        map.insert(exp.experiment_id.clone(), exp.clone());
        exp
    }

    pub fn get(&self, id: &str) -> Option<Experiment> {
        self.experiments.read().unwrap().get(id).cloned()
    }

    pub fn list(&self, state_filter: Option<i32>) -> Vec<Experiment> {
        let map = self.experiments.read().unwrap();
        map.values()
            .filter(|e| {
                state_filter
                    .map(|s| s == 0 || e.state == s)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    pub fn update_state(&self, id: &str, new_state: ExperimentState) -> Option<Experiment> {
        let mut map = self.experiments.write().unwrap();
        let mut ver = self.version.write().unwrap();
        if let Some(exp) = map.get_mut(id) {
            exp.state = new_state as i32;
            *ver += 1;
            Some(exp.clone())
        } else {
            None
        }
    }

    pub fn current_version(&self) -> i64 {
        *self.version.read().unwrap()
    }

    pub fn insert_layer(&self, layer: Layer) -> Layer {
        let mut map = self.layers.write().unwrap();
        map.insert(layer.layer_id.clone(), layer.clone());
        layer
    }

    pub fn get_layer(&self, id: &str) -> Option<Layer> {
        self.layers.read().unwrap().get(id).cloned()
    }
}

// ---------------------------------------------------------------------------
// In-memory metric definition store (ADR-026 Phase 1)
//
// Parallel to `ExperimentStore` above. The PG-backed implementation lives in
// `crate::store::ManagementStore`; the contract tests use this DB-free copy
// so wire-format round-trips can be exercised without a database. Method names
// match the PG store one-for-one so test fixtures swap in trivially.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct MetricStore {
    // Keyed by metric_id; insertion-order is not preserved (callers sort by id
    // when comparing — same behaviour as the PG `ORDER BY metric_id`).
    metrics: Arc<RwLock<HashMap<String, MetricDefinition>>>,
}

impl MetricStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_metric(&self, metric: MetricDefinition) -> Result<MetricDefinition, Box<Status>> {
        let mut map = self.metrics.write().unwrap();
        if map.contains_key(&metric.metric_id) {
            return Err(Box::new(Status::already_exists(metric.metric_id.clone())));
        }
        map.insert(metric.metric_id.clone(), metric.clone());
        Ok(metric)
    }

    pub fn get_metric(&self, metric_id: &str) -> Option<MetricDefinition> {
        self.metrics.read().unwrap().get(metric_id).cloned()
    }

    pub fn list_metrics(&self, type_filter: Option<MetricType>) -> Vec<MetricDefinition> {
        let map = self.metrics.read().unwrap();
        let mut out: Vec<MetricDefinition> = map
            .values()
            .filter(|m| match type_filter {
                None | Some(MetricType::Unspecified) => true,
                Some(t) => m.r#type == t as i32,
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.metric_id.cmp(&b.metric_id));
        out
    }

    pub fn exists_metric(&self, metric_id: &str) -> bool {
        self.metrics.read().unwrap().contains_key(metric_id)
    }

    pub fn exists_all_metrics(&self, metric_ids: &[&str]) -> bool {
        let map = self.metrics.read().unwrap();
        metric_ids.iter().all(|id| map.contains_key(*id))
    }

    /// Walk a COMPOSITE row and return the operand metric_ids in declaration
    /// order. Returns an empty Vec if the metric is missing or not COMPOSITE.
    pub fn get_composite_operands(&self, metric_id: &str) -> Vec<String> {
        let map = self.metrics.read().unwrap();
        let Some(m) = map.get(metric_id) else {
            return Vec::new();
        };
        match m.type_config.as_ref() {
            Some(MetricTypeConfig::Composite(cfg)) => {
                cfg.operands.iter().map(|op| op.metric_id.clone()).collect()
            }
            _ => Vec::new(),
        }
    }
}

// `MetricLookup` bridges the in-memory store to the cycle-detection validator.
// Method names + semantics mirror the PG-backed `ManagementStore` so the
// validator implementation stays storage-agnostic.
#[tonic::async_trait]
impl crate::validators::MetricLookup for MetricStore {
    async fn exists_all_metrics(
        &self,
        metric_ids: &[&str],
    ) -> Result<bool, crate::store::StoreError> {
        Ok(MetricStore::exists_all_metrics(self, metric_ids))
    }

    async fn get_composite_operands(
        &self,
        metric_id: &str,
    ) -> Result<Vec<String>, crate::store::StoreError> {
        let map = self.metrics.read().unwrap();
        let Some(m) = map.get(metric_id) else {
            return Err(crate::store::StoreError::NotFound(metric_id.to_string()));
        };
        match m.type_config.as_ref() {
            Some(MetricTypeConfig::Composite(cfg)) => {
                Ok(cfg.operands.iter().map(|op| op.metric_id.clone()).collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn get_metricql_refs(
        &self,
        metric_id: &str,
    ) -> Result<Vec<String>, crate::store::StoreError> {
        let map = self.metrics.read().unwrap();
        let Some(m) = map.get(metric_id) else {
            return Err(crate::store::StoreError::NotFound(metric_id.to_string()));
        };
        if m.r#type != MetricType::Metricql as i32 {
            return Ok(Vec::new());
        }
        let expr = m.metricql_expression.trim();
        if expr.is_empty() {
            return Ok(Vec::new());
        }
        use crate::validators::metricql;
        use crate::validators::metricql::analyze;
        let ast = match metricql::parse_only(expr) {
            Ok(node) => node,
            Err(_) => return Ok(Vec::new()),
        };
        Ok(analyze::extract_metric_refs(&ast))
    }

    async fn get_metric_type(
        &self,
        metric_id: &str,
    ) -> Result<MetricType, crate::store::StoreError> {
        let map = self.metrics.read().unwrap();
        let Some(m) = map.get(metric_id) else {
            return Err(crate::store::StoreError::NotFound(metric_id.to_string()));
        };
        Ok(MetricType::try_from(m.r#type).unwrap_or(MetricType::Unspecified))
    }
}

// ---------------------------------------------------------------------------
// Contract test handler
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ManagementServiceHandler {
    store: Arc<ExperimentStore>,
    metric_store: Arc<MetricStore>,
}

impl ManagementServiceHandler {
    pub fn new(store: Arc<ExperimentStore>) -> Self {
        Self {
            store,
            metric_store: Arc::new(MetricStore::new()),
        }
    }

    /// Construct with caller-supplied metric store. Lets contract tests share
    /// a single `MetricStore` across multiple handler instances.
    pub fn with_metric_store(
        store: Arc<ExperimentStore>,
        metric_store: Arc<MetricStore>,
    ) -> Self {
        Self { store, metric_store }
    }

    /// Accessor so contract tests can pre-seed metrics directly.
    pub fn metric_store(&self) -> &Arc<MetricStore> {
        &self.metric_store
    }

    #[allow(clippy::result_large_err)]
    fn validate_create(exp: &Experiment) -> Result<(), Status> {
        if exp.name.is_empty() {
            return Err(Status::invalid_argument("experiment.name is required"));
        }
        if exp.owner_email.is_empty() {
            return Err(Status::invalid_argument("experiment.owner_email is required"));
        }
        if exp.layer_id.is_empty() {
            return Err(Status::invalid_argument("experiment.layer_id is required"));
        }
        if exp.primary_metric_id.is_empty() {
            return Err(Status::invalid_argument(
                "experiment.primary_metric_id is required",
            ));
        }
        if exp.variants.len() < 2 {
            return Err(Status::invalid_argument(
                "experiment must have at least 2 variants",
            ));
        }
        let control_count = exp.variants.iter().filter(|v| v.is_control).count();
        if control_count != 1 {
            return Err(Status::invalid_argument(
                "experiment must have exactly one control variant",
            ));
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn validate_transition(
        exp: &Experiment,
        expected_current: ExperimentState,
        target: ExperimentState,
    ) -> Result<(), Status> {
        if exp.state != expected_current as i32 {
            return Err(Status::failed_precondition(format!(
                "experiment {} is in state {:?}, expected {:?}",
                exp.experiment_id, exp.state, expected_current
            )));
        }
        let _ = target;
        Ok(())
    }
}

#[tonic::async_trait]
impl ExperimentManagementService for ManagementServiceHandler {
    async fn create_experiment(
        &self,
        request: Request<CreateExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        let mut exp = req
            .experiment
            .ok_or_else(|| Status::invalid_argument("experiment is required"))?;

        Self::validate_create(&exp)?;

        exp.experiment_id = Uuid::new_v4().to_string();
        exp.hash_salt = Uuid::new_v4().to_string();
        exp.state = ExperimentState::Draft as i32;

        for v in &mut exp.variants {
            if v.variant_id.is_empty() {
                v.variant_id = Uuid::new_v4().to_string();
            }
        }

        let created = self.store.insert(exp);
        Ok(Response::new(created))
    }

    async fn get_experiment(
        &self,
        request: Request<GetExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        self.store
            .get(&req.experiment_id)
            .map(Response::new)
            .ok_or_else(|| Status::not_found(format!("experiment {} not found", req.experiment_id)))
    }

    async fn update_experiment(
        &self,
        request: Request<UpdateExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        let exp = req
            .experiment
            .ok_or_else(|| Status::invalid_argument("experiment is required"))?;
        if exp.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment.experiment_id is required"));
        }
        self.store
            .get(&exp.experiment_id)
            .ok_or_else(|| Status::not_found("experiment not found"))?;

        let updated = self.store.insert(exp);
        Ok(Response::new(updated))
    }

    async fn list_experiments(
        &self,
        request: Request<ListExperimentsRequest>,
    ) -> Result<Response<ListExperimentsResponse>, Status> {
        let req = request.into_inner();
        let state_filter = if req.state_filter == 0 {
            None
        } else {
            Some(req.state_filter)
        };
        let experiments = self.store.list(state_filter);
        Ok(Response::new(ListExperimentsResponse {
            experiments,
            next_page_token: String::new(),
        }))
    }

    async fn start_experiment(
        &self,
        request: Request<StartExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let exp = self
            .store
            .get(&req.experiment_id)
            .ok_or_else(|| Status::not_found("experiment not found"))?;

        Self::validate_transition(
            &exp,
            ExperimentState::Draft,
            ExperimentState::Running,
        )?;

        if exp.r#type == ExperimentType::Unspecified as i32 {
            return Err(Status::failed_precondition(
                "experiment type must be set before starting",
            ));
        }

        self.store
            .update_state(&req.experiment_id, ExperimentState::Running)
            .map(Response::new)
            .ok_or_else(|| Status::internal("failed to update state"))
    }

    async fn conclude_experiment(
        &self,
        request: Request<ConcludeExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let exp = self
            .store
            .get(&req.experiment_id)
            .ok_or_else(|| Status::not_found("experiment not found"))?;

        Self::validate_transition(
            &exp,
            ExperimentState::Running,
            ExperimentState::Concluded,
        )?;

        self.store
            .update_state(&req.experiment_id, ExperimentState::Concluded)
            .map(Response::new)
            .ok_or_else(|| Status::internal("failed to update state"))
    }

    async fn archive_experiment(
        &self,
        request: Request<ArchiveExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let exp = self
            .store
            .get(&req.experiment_id)
            .ok_or_else(|| Status::not_found("experiment not found"))?;

        Self::validate_transition(
            &exp,
            ExperimentState::Concluded,
            ExperimentState::Archived,
        )?;

        self.store
            .update_state(&req.experiment_id, ExperimentState::Archived)
            .map(Response::new)
            .ok_or_else(|| Status::internal("failed to update state"))
    }

    async fn pause_experiment(
        &self,
        request: Request<PauseExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let exp = self
            .store
            .get(&req.experiment_id)
            .ok_or_else(|| Status::not_found("experiment not found"))?;

        if exp.state != ExperimentState::Running as i32 {
            return Err(Status::failed_precondition(
                "can only pause RUNNING experiments",
            ));
        }
        Ok(Response::new(exp))
    }

    async fn resume_experiment(
        &self,
        request: Request<ResumeExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();
        if req.experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let exp = self
            .store
            .get(&req.experiment_id)
            .ok_or_else(|| Status::not_found("experiment not found"))?;

        if exp.state != ExperimentState::Running as i32 {
            return Err(Status::failed_precondition(
                "can only resume RUNNING (paused) experiments",
            ));
        }
        Ok(Response::new(exp))
    }

    async fn create_metric_definition(
        &self,
        request: Request<CreateMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        let mut metric = request
            .into_inner()
            .metric
            .ok_or_else(|| Status::invalid_argument("metric is required"))?;

        // Mirror the production handler flow: validate via the shared
        // skeleton, then write to the (in-memory) store. The validator
        // enforces non-empty metric_id + name; callers in contract tests
        // that want a server-minted id should pass an empty string and let
        // the helper below stamp a UUID.
        if metric.metric_id.is_empty() {
            metric.metric_id = Uuid::new_v4().to_string();
        }
        crate::validators::validate_metric_definition(&metric, self.metric_store.as_ref())
            .await
            .map_err(|boxed| *boxed)?;

        let created = self.metric_store.create_metric(metric).map_err(|boxed| *boxed)?;
        Ok(Response::new(created))
    }

    async fn get_metric_definition(
        &self,
        request: Request<GetMetricDefinitionRequest>,
    ) -> Result<Response<MetricDefinition>, Status> {
        let req = request.into_inner();
        if req.metric_id.is_empty() {
            return Err(Status::invalid_argument("metric_id is required"));
        }
        self.metric_store
            .get_metric(&req.metric_id)
            .map(Response::new)
            .ok_or_else(|| Status::not_found(format!("metric {} not found", req.metric_id)))
    }

    async fn list_metric_definitions(
        &self,
        request: Request<ListMetricDefinitionsRequest>,
    ) -> Result<Response<ListMetricDefinitionsResponse>, Status> {
        let req = request.into_inner();
        let type_filter = MetricType::try_from(req.type_filter)
            .ok()
            .filter(|t| *t != MetricType::Unspecified);
        let metrics = self.metric_store.list_metrics(type_filter);
        Ok(Response::new(ListMetricDefinitionsResponse {
            metrics,
            next_page_token: String::new(),
        }))
    }

    async fn create_layer(
        &self,
        request: Request<CreateLayerRequest>,
    ) -> Result<Response<Layer>, Status> {
        let req = request.into_inner();
        let mut layer = req
            .layer
            .ok_or_else(|| Status::invalid_argument("layer is required"))?;
        if layer.name.is_empty() {
            return Err(Status::invalid_argument("layer.name is required"));
        }
        layer.layer_id = Uuid::new_v4().to_string();
        if layer.total_buckets == 0 {
            layer.total_buckets = 10_000;
        }
        let created = self.store.insert_layer(layer);
        Ok(Response::new(created))
    }

    async fn get_layer(
        &self,
        request: Request<GetLayerRequest>,
    ) -> Result<Response<Layer>, Status> {
        let req = request.into_inner();
        if req.layer_id.is_empty() {
            return Err(Status::invalid_argument("layer_id is required"));
        }
        self.store
            .get_layer(&req.layer_id)
            .map(Response::new)
            .ok_or_else(|| Status::not_found(format!("layer {} not found", req.layer_id)))
    }

    async fn get_layer_allocations(
        &self,
        request: Request<GetLayerAllocationsRequest>,
    ) -> Result<Response<GetLayerAllocationsResponse>, Status> {
        let req = request.into_inner();
        if req.layer_id.is_empty() {
            return Err(Status::invalid_argument("layer_id is required"));
        }
        Ok(Response::new(GetLayerAllocationsResponse {
            allocations: vec![],
        }))
    }

    async fn create_targeting_rule(
        &self,
        request: Request<CreateTargetingRuleRequest>,
    ) -> Result<Response<TargetingRule>, Status> {
        let req = request.into_inner();
        let mut rule = req
            .rule
            .ok_or_else(|| Status::invalid_argument("rule is required"))?;
        rule.rule_id = Uuid::new_v4().to_string();
        Ok(Response::new(rule))
    }

    async fn create_surrogate_model(
        &self,
        request: Request<CreateSurrogateModelRequest>,
    ) -> Result<Response<experimentation_proto::experimentation::common::v1::SurrogateModelConfig>, Status> {
        let req = request.into_inner();
        let mut model = req
            .model
            .ok_or_else(|| Status::invalid_argument("model is required"))?;
        model.model_id = Uuid::new_v4().to_string();
        Ok(Response::new(model))
    }

    async fn list_surrogate_models(
        &self,
        _request: Request<ListSurrogateModelsRequest>,
    ) -> Result<Response<ListSurrogateModelsResponse>, Status> {
        Ok(Response::new(ListSurrogateModelsResponse {
            models: vec![],
            next_page_token: String::new(),
        }))
    }

    async fn get_surrogate_calibration(
        &self,
        request: Request<GetSurrogateCalibrationRequest>,
    ) -> Result<Response<experimentation_proto::experimentation::common::v1::SurrogateModelConfig>, Status> {
        let req = request.into_inner();
        Err(Status::not_found(format!(
            "surrogate model {} not found",
            req.model_id
        )))
    }

    async fn trigger_surrogate_recalibration(
        &self,
        request: Request<TriggerSurrogateRecalibrationRequest>,
    ) -> Result<Response<()>, Status> {
        let _req = request.into_inner();
        Ok(Response::new(()))
    }

    type StreamConfigUpdatesStream =
        Pin<Box<dyn Stream<Item = Result<ConfigUpdateEvent, Status>> + Send>>;

    async fn stream_config_updates(
        &self,
        _request: Request<StreamConfigUpdatesRequest>,
    ) -> Result<Response<Self::StreamConfigUpdatesStream>, Status> {
        // Contract test stub: return an empty stream.
        let stream = tokio_stream::empty();
        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_portfolio_allocation(
        &self,
        _request: Request<GetPortfolioAllocationRequest>,
    ) -> Result<Response<GetPortfolioAllocationResponse>, Status> {
        Err(Status::unimplemented("GetPortfolioAllocation not yet implemented in Rust M5"))
    }

    async fn validate_metricql(
        &self,
        request: Request<ValidateMetricqlRequest>,
    ) -> Result<Response<ValidateMetricqlResponse>, Status> {
        let req = request.into_inner();
        if req.experiment_id.trim().is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        // Contract test stub: delegate to the validator directly (no DB needed).
        use crate::validators::metricql::{validate_metricql as vm, ValidateContext};
        if req.metricql_expression.trim().is_empty() {
            return Ok(Response::new(ValidateMetricqlResponse {
                diagnostics: vec![
                    experimentation_proto::experimentation::common::v1::MetricqlDiagnostic {
                        severity: experimentation_proto::experimentation::common::v1::metricql_diagnostic::Severity::Error as i32,
                        message: "empty MetricQL expression".to_string(),
                        span: Some(experimentation_proto::experimentation::common::v1::metricql_diagnostic::Span {
                            start_offset: 0,
                            end_offset: 0,
                            line: 1,
                            column: 1,
                        }),
                    },
                ],
                referenced_metric_ids: vec![],
            }));
        }
        let ctx = ValidateContext { known_metric_ids: None };
        match vm(&req.metricql_expression, &ctx) {
            Ok(refs) => Ok(Response::new(ValidateMetricqlResponse {
                diagnostics: vec![],
                referenced_metric_ids: refs,
            })),
            Err(_diags) => Ok(Response::new(ValidateMetricqlResponse {
                diagnostics: vec![
                    experimentation_proto::experimentation::common::v1::MetricqlDiagnostic {
                        severity: experimentation_proto::experimentation::common::v1::metricql_diagnostic::Severity::Error as i32,
                        message: "validation failed".to_string(),
                        span: None,
                    },
                ],
                referenced_metric_ids: vec![],
            })),
        }
    }
}
