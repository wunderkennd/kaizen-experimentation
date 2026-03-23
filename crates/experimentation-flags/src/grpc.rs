//! tonic gRPC service implementation for FeatureFlagService (M7 Rust port).
//!
//! Phase 1: Flag CRUD + EvaluateFlag/EvaluateFlags.
//! Phase 2: PromoteToExperiment (M5 tonic client), audit trail writes.

use std::sync::Arc;

use tonic::transport::Channel;
use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::{
    BanditAlgorithm, BanditArm, BanditConfig, Experiment, ExperimentState, ExperimentType,
    InterleavingConfig, InterleavingMethod, CreditAssignment, SessionConfig, Variant,
};
use experimentation_proto::experimentation::flags::v1::{
    feature_flag_service_server::{FeatureFlagService, FeatureFlagServiceServer},
    CreateFlagRequest, EvaluateFlagRequest, EvaluateFlagResponse, EvaluateFlagsRequest,
    EvaluateFlagsResponse, Flag as ProtoFlag, FlagType, FlagVariant as ProtoFlagVariant,
    GetFlagRequest, ListFlagsRequest, ListFlagsResponse, PromoteToExperimentRequest,
    UpdateFlagRequest,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_client::ExperimentManagementServiceClient,
    CreateExperimentRequest,
};

use crate::audit::AuditStore;
use crate::config::FlagsConfig;
use crate::store::{Flag, FlagStore, FlagVariant, StoreError};

// ---------------------------------------------------------------------------
// Service handler
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FlagsServiceHandler {
    store: Arc<FlagStore>,
    audit: Option<Arc<AuditStore>>,
    m5_client: Option<ExperimentManagementServiceClient<Channel>>,
    default_layer_id: String,
}

impl FlagsServiceHandler {
    pub fn new(store: FlagStore, config: &FlagsConfig) -> Self {
        Self {
            store: Arc::new(store),
            audit: None,
            m5_client: None,
            default_layer_id: config.default_layer_id.clone(),
        }
    }

    pub fn with_audit(mut self, audit: Arc<AuditStore>) -> Self {
        self.audit = Some(audit);
        self
    }

    pub fn with_m5_client(mut self, client: ExperimentManagementServiceClient<Channel>) -> Self {
        self.m5_client = Some(client);
        self
    }

    pub fn store(&self) -> Arc<FlagStore> {
        self.store.clone()
    }

    pub fn audit(&self) -> Option<Arc<AuditStore>> {
        self.audit.clone()
    }
}

// ---------------------------------------------------------------------------
// Proto ↔ domain conversions (unchanged from Phase 1)
// ---------------------------------------------------------------------------

fn flag_type_to_str(t: i32) -> &'static str {
    match FlagType::try_from(t).unwrap_or(FlagType::Unspecified) {
        FlagType::Boolean => "BOOLEAN",
        FlagType::String => "STRING",
        FlagType::Numeric => "NUMERIC",
        FlagType::Json => "JSON",
        FlagType::Unspecified => "BOOLEAN",
    }
}

fn str_to_flag_type(s: &str) -> i32 {
    match s {
        "BOOLEAN" => FlagType::Boolean as i32,
        "STRING" => FlagType::String as i32,
        "NUMERIC" => FlagType::Numeric as i32,
        "JSON" => FlagType::Json as i32,
        _ => FlagType::Unspecified as i32,
    }
}

fn domain_to_proto(f: &Flag) -> ProtoFlag {
    ProtoFlag {
        flag_id: f.flag_id.to_string(),
        name: f.name.clone(),
        description: f.description.clone(),
        r#type: str_to_flag_type(&f.flag_type),
        default_value: f.default_value.clone(),
        enabled: f.enabled,
        rollout_percentage: f.rollout_percentage,
        targeting_rule_id: f
            .targeting_rule_id
            .map(|u| u.to_string())
            .unwrap_or_default(),
        variants: f
            .variants
            .iter()
            .map(|v| ProtoFlagVariant {
                variant_id: v.variant_id.to_string(),
                value: v.value.clone(),
                traffic_fraction: v.traffic_fraction,
            })
            .collect(),
    }
}

#[allow(clippy::result_large_err)]
fn proto_to_domain(pb: &ProtoFlag) -> Result<Flag, Status> {
    let flag_id = if pb.flag_id.is_empty() {
        Uuid::nil()
    } else {
        Uuid::parse_str(&pb.flag_id)
            .map_err(|_| Status::invalid_argument("invalid flag_id UUID"))?
    };

    let targeting_rule_id = if pb.targeting_rule_id.is_empty() {
        None
    } else {
        Some(
            Uuid::parse_str(&pb.targeting_rule_id)
                .map_err(|_| Status::invalid_argument("invalid targeting_rule_id UUID"))?,
        )
    };

    let variants: Vec<FlagVariant> = pb
        .variants
        .iter()
        .map(|v| FlagVariant {
            variant_id: Uuid::nil(),
            flag_id,
            value: v.value.clone(),
            traffic_fraction: v.traffic_fraction,
            ordinal: 0,
        })
        .collect();

    Ok(Flag {
        flag_id,
        name: pb.name.clone(),
        description: pb.description.clone(),
        flag_type: flag_type_to_str(pb.r#type).to_string(),
        default_value: pb.default_value.clone(),
        enabled: pb.enabled,
        rollout_percentage: pb.rollout_percentage,
        salt: String::new(),
        targeting_rule_id,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        promoted_experiment_id: None,
        promoted_at: None,
        resolved_at: None,
        variants,
    })
}

#[allow(clippy::result_large_err)]
fn validate_flag(pb: &ProtoFlag) -> Result<(), Status> {
    if pb.name.trim().is_empty() {
        return Err(Status::invalid_argument("name is required"));
    }
    if pb.r#type == FlagType::Unspecified as i32 {
        return Err(Status::invalid_argument("type must be specified"));
    }
    if pb.rollout_percentage < 0.0 || pb.rollout_percentage > 1.0 {
        return Err(Status::invalid_argument(
            "rollout_percentage must be between 0.0 and 1.0",
        ));
    }
    if pb.r#type == FlagType::Boolean as i32 {
        let dv = pb.default_value.as_str();
        if dv != "true" && dv != "false" {
            return Err(Status::invalid_argument(
                r#"boolean flag default_value must be "true" or "false""#,
            ));
        }
    }
    if !pb.variants.is_empty() {
        let sum: f64 = pb.variants.iter().map(|v| v.traffic_fraction).sum();
        if (sum - 1.0).abs() > 0.001 {
            return Err(Status::invalid_argument(format!(
                "variant traffic_fractions must sum to 1.0 (got {sum:.6})"
            )));
        }
        for v in &pb.variants {
            if v.traffic_fraction < 0.0 || v.traffic_fraction > 1.0 {
                return Err(Status::invalid_argument(
                    "variant traffic_fraction must be between 0.0 and 1.0",
                ));
            }
        }
    }
    Ok(())
}

fn store_err_to_status(e: StoreError) -> Status {
    match e {
        StoreError::NotFound(msg) => Status::not_found(msg),
        StoreError::AlreadyExists(msg) => Status::already_exists(msg),
        StoreError::InvalidPageToken => Status::invalid_argument("invalid page token"),
        StoreError::Db(e) => {
            warn!(error = %e, "database error");
            Status::internal(format!("database error: {e}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Audit helpers
// ---------------------------------------------------------------------------

fn flag_snapshot(f: &Flag) -> serde_json::Value {
    serde_json::json!({
        "name": f.name,
        "description": f.description,
        "type": f.flag_type,
        "default_value": f.default_value,
        "enabled": f.enabled,
        "rollout_percentage": f.rollout_percentage,
        "targeting_rule_id": f.targeting_rule_id.map(|u| u.to_string()),
        "variant_count": f.variants.len(),
    })
}

async fn record_audit_nonfatal(
    audit: &Option<Arc<AuditStore>>,
    flag_id: Uuid,
    action: &str,
    actor_email: &str,
    previous: Option<&Flag>,
    current: Option<&Flag>,
) {
    let Some(audit) = audit else { return };
    let prev_val = previous
        .map(flag_snapshot)
        .unwrap_or(serde_json::Value::Null);
    let new_val = current
        .map(flag_snapshot)
        .unwrap_or(serde_json::Value::Null);
    if let Err(e) = audit
        .record_audit(flag_id, action, actor_email, &prev_val, &new_val)
        .await
    {
        warn!(error = %e, %flag_id, %action, "audit write failed (non-fatal)");
    }
}

// ---------------------------------------------------------------------------
// Evaluation logic — direct call to experimentation_hash::bucket()
// No CGo, no FFI. Hash parity guaranteed by construction.
// ---------------------------------------------------------------------------

fn evaluate_flag(f: &Flag, user_id: &str) -> (String, String) {
    if !f.enabled {
        return (f.default_value.clone(), String::new());
    }

    let bucket = experimentation_hash::bucket(user_id, &f.salt, 10_000);
    let threshold = (f.rollout_percentage * 10_000.0) as u32;

    if bucket >= threshold {
        return (f.default_value.clone(), String::new());
    }

    if f.variants.is_empty() {
        if f.flag_type == "BOOLEAN" {
            return ("true".to_string(), String::new());
        }
        return (f.default_value.clone(), String::new());
    }

    // Multi-variant: assign based on cumulative traffic_fraction.
    let bucket_fraction = bucket as f64 / 10_000.0;
    let mut cumulative = 0.0;
    for v in &f.variants {
        cumulative += v.traffic_fraction;
        if bucket_fraction < cumulative {
            return (v.value.clone(), v.variant_id.to_string());
        }
    }

    // Fallback to last variant (handles float rounding).
    let last = f.variants.last().unwrap();
    (last.value.clone(), last.variant_id.to_string())
}

// ---------------------------------------------------------------------------
// PromoteToExperiment helpers (Phase 2)
// ---------------------------------------------------------------------------

fn build_variants(f: &Flag) -> Vec<Variant> {
    if !f.variants.is_empty() {
        return f
            .variants
            .iter()
            .enumerate()
            .map(|(i, v)| Variant {
                name: format!("variant_{i}"),
                traffic_fraction: v.traffic_fraction,
                is_control: i == 0,
                payload_json: format!(r#"{{"value": {:?}}}"#, v.value),
                variant_id: v.variant_id.to_string(),
            })
            .collect();
    }

    vec![
        Variant {
            name: "control".into(),
            traffic_fraction: 1.0 - f.rollout_percentage,
            is_control: true,
            payload_json: r#"{"value": "false"}"#.into(),
            variant_id: String::new(),
        },
        Variant {
            name: "treatment".into(),
            traffic_fraction: f.rollout_percentage,
            is_control: false,
            payload_json: r#"{"value": "true"}"#.into(),
            variant_id: String::new(),
        },
    ]
}

fn apply_type_config(exp: &mut Experiment, f: &Flag) -> Result<(), Status> {
    let exp_type = ExperimentType::try_from(exp.r#type).unwrap_or(ExperimentType::Unspecified);

    match exp_type {
        ExperimentType::Ab
        | ExperimentType::Multivariate
        | ExperimentType::PlaybackQoe
        | ExperimentType::CumulativeHoldout => {
            if exp_type == ExperimentType::CumulativeHoldout {
                exp.is_cumulative_holdout = true;
            }
        }

        ExperimentType::Interleaving => {
            if f.variants.len() < 2 {
                return Err(Status::invalid_argument(
                    "interleaving requires at least 2 variants (algorithm_ids)",
                ));
            }
            let algorithm_ids: Vec<String> = f.variants.iter().map(|v| v.value.clone()).collect();
            exp.interleaving_config = Some(InterleavingConfig {
                method: InterleavingMethod::TeamDraft as i32,
                algorithm_ids,
                max_list_size: 50,
                credit_assignment: CreditAssignment::BinaryWin as i32,
                credit_metric_event: String::new(),
            });
        }

        ExperimentType::SessionLevel => {
            exp.session_config = Some(SessionConfig {
                session_id_attribute: "session_id".into(),
                allow_cross_session_variation: true,
                min_sessions_per_user: 0,
            });
        }

        ExperimentType::Mab => {
            if f.variants.len() < 2 {
                return Err(Status::invalid_argument(
                    "MAB requires at least 2 variants (arms)",
                ));
            }
            exp.bandit_config = Some(build_bandit_config(BanditAlgorithm::ThompsonSampling, f));
        }

        ExperimentType::ContextualBandit => {
            if f.variants.len() < 2 {
                return Err(Status::invalid_argument(
                    "contextual bandit requires at least 2 variants (arms)",
                ));
            }
            exp.bandit_config = Some(build_bandit_config(BanditAlgorithm::LinearUcb, f));
        }

        _ => {}
    }

    Ok(())
}

fn build_bandit_config(algo: BanditAlgorithm, f: &Flag) -> BanditConfig {
    let arms: Vec<BanditArm> = f
        .variants
        .iter()
        .map(|v| BanditArm {
            arm_id: v.variant_id.to_string(),
            name: v.value.clone(),
            payload_json: format!(r#"{{"value": {:?}}}"#, v.value),
        })
        .collect();

    BanditConfig {
        algorithm: algo as i32,
        arms,
        min_exploration_fraction: 0.1,
        reward_metric_id: String::new(),
        context_feature_keys: vec![],
        warmup_observations: 1000,
        cold_start_window: None,
        reward_objectives: vec![],
        composition_method: 0,
        arm_constraints: vec![],
        global_constraints: vec![],
        slate_config: None,
        mad_randomization_fraction: 0.0,
    }
}

// ---------------------------------------------------------------------------
// tonic service implementation
// ---------------------------------------------------------------------------

#[tonic::async_trait]
impl FeatureFlagService for FlagsServiceHandler {
    async fn create_flag(
        &self,
        request: Request<CreateFlagRequest>,
    ) -> Result<Response<ProtoFlag>, Status> {
        let pb = request
            .into_inner()
            .flag
            .ok_or_else(|| Status::invalid_argument("flag is required"))?;

        validate_flag(&pb)?;

        let domain = proto_to_domain(&pb)?;
        let created = self
            .store
            .create_flag(&domain)
            .await
            .map_err(store_err_to_status)?;

        info!(flag_id = %created.flag_id, name = %created.name, "flag created");
        record_audit_nonfatal(&self.audit, created.flag_id, "create", "system", None, Some(&created)).await;

        Ok(Response::new(domain_to_proto(&created)))
    }

    async fn get_flag(
        &self,
        request: Request<GetFlagRequest>,
    ) -> Result<Response<ProtoFlag>, Status> {
        let flag_id_str = request.into_inner().flag_id;
        if flag_id_str.is_empty() {
            return Err(Status::invalid_argument("flag_id is required"));
        }

        let flag_id = Uuid::parse_str(&flag_id_str)
            .map_err(|_| Status::invalid_argument("invalid flag_id UUID"))?;

        let flag = self
            .store
            .get_flag(flag_id)
            .await
            .map_err(store_err_to_status)?;

        Ok(Response::new(domain_to_proto(&flag)))
    }

    async fn update_flag(
        &self,
        request: Request<UpdateFlagRequest>,
    ) -> Result<Response<ProtoFlag>, Status> {
        let pb = request
            .into_inner()
            .flag
            .ok_or_else(|| Status::invalid_argument("flag is required"))?;

        if pb.flag_id.is_empty() {
            return Err(Status::invalid_argument("flag_id is required"));
        }

        validate_flag(&pb)?;

        // Fetch previous state for audit delta.
        let previous = if self.audit.is_some() {
            let flag_id = Uuid::parse_str(&pb.flag_id).ok();
            if let Some(id) = flag_id {
                self.store.get_flag(id).await.ok()
            } else {
                None
            }
        } else {
            None
        };

        let domain = proto_to_domain(&pb)?;
        let updated = self
            .store
            .update_flag(&domain)
            .await
            .map_err(store_err_to_status)?;

        // Determine specific audit action.
        let action = if let Some(ref prev) = previous {
            if prev.enabled != updated.enabled {
                if updated.enabled { "enable" } else { "disable" }
            } else if (prev.rollout_percentage - updated.rollout_percentage).abs() > 1e-9 {
                "rollout_change"
            } else {
                "update"
            }
        } else {
            "update"
        };

        info!(flag_id = %updated.flag_id, %action, "flag updated");
        record_audit_nonfatal(
            &self.audit,
            updated.flag_id,
            action,
            "system",
            previous.as_ref(),
            Some(&updated),
        )
        .await;

        Ok(Response::new(domain_to_proto(&updated)))
    }

    async fn list_flags(
        &self,
        request: Request<ListFlagsRequest>,
    ) -> Result<Response<ListFlagsResponse>, Status> {
        let req = request.into_inner();
        let (flags, next_token) = self
            .store
            .list_flags(req.page_size as i64, &req.page_token)
            .await
            .map_err(store_err_to_status)?;

        Ok(Response::new(ListFlagsResponse {
            flags: flags.iter().map(domain_to_proto).collect(),
            next_page_token: next_token,
        }))
    }

    async fn evaluate_flag(
        &self,
        request: Request<EvaluateFlagRequest>,
    ) -> Result<Response<EvaluateFlagResponse>, Status> {
        let req = request.into_inner();
        if req.flag_id.is_empty() {
            return Err(Status::invalid_argument("flag_id is required"));
        }
        if req.user_id.is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }

        let flag_id = Uuid::parse_str(&req.flag_id)
            .map_err(|_| Status::invalid_argument("invalid flag_id UUID"))?;

        let flag = self
            .store
            .get_flag(flag_id)
            .await
            .map_err(store_err_to_status)?;

        let (value, variant_id) = evaluate_flag(&flag, &req.user_id);

        Ok(Response::new(EvaluateFlagResponse {
            flag_id: req.flag_id,
            value,
            variant_id,
        }))
    }

    async fn evaluate_flags(
        &self,
        request: Request<EvaluateFlagsRequest>,
    ) -> Result<Response<EvaluateFlagsResponse>, Status> {
        let req = request.into_inner();
        if req.user_id.is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }

        let flags = self
            .store
            .get_all_enabled_flags()
            .await
            .map_err(store_err_to_status)?;

        let evaluations = flags
            .iter()
            .map(|f| {
                let (value, variant_id) = evaluate_flag(f, &req.user_id);
                EvaluateFlagResponse {
                    flag_id: f.flag_id.to_string(),
                    value,
                    variant_id,
                }
            })
            .collect();

        Ok(Response::new(EvaluateFlagsResponse { evaluations }))
    }

    /// Phase 2: promote a flag to a tracked experiment via M5 CreateExperiment.
    async fn promote_to_experiment(
        &self,
        request: Request<PromoteToExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        let req = request.into_inner();

        if req.flag_id.is_empty() {
            return Err(Status::invalid_argument("flag_id is required"));
        }
        if req.primary_metric_id.is_empty() {
            return Err(Status::invalid_argument("primary_metric_id is required"));
        }
        let exp_type = ExperimentType::try_from(req.experiment_type)
            .unwrap_or(ExperimentType::Unspecified);
        if exp_type == ExperimentType::Unspecified {
            return Err(Status::invalid_argument("experiment_type is required"));
        }

        let flag_id = Uuid::parse_str(&req.flag_id)
            .map_err(|_| Status::invalid_argument("invalid flag_id UUID"))?;

        let flag = self
            .store
            .get_flag(flag_id)
            .await
            .map_err(store_err_to_status)?;

        if !flag.enabled {
            return Err(Status::failed_precondition(
                "flag must be enabled to promote to experiment",
            ));
        }

        let variants = build_variants(&flag);
        let mut experiment = Experiment {
            name: format!("Promoted from flag: {}", flag.name),
            description: format!(
                "Auto-promoted from feature flag {} ({})",
                flag.name, flag.flag_id
            ),
            owner_email: "system".into(),
            r#type: exp_type as i32,
            layer_id: self.default_layer_id.clone(),
            variants,
            primary_metric_id: req.primary_metric_id.clone(),
            secondary_metric_ids: req.secondary_metric_ids.clone(),
            targeting_rule_id: flag
                .targeting_rule_id
                .map(|u| u.to_string())
                .unwrap_or_default(),
            hash_salt: flag.salt.clone(),
            ..Default::default()
        };

        apply_type_config(&mut experiment, &flag)?;

        let result = match &self.m5_client {
            Some(client) => {
                let mut client = client.clone();
                let resp = client
                    .create_experiment(CreateExperimentRequest {
                        experiment: Some(experiment),
                    })
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "M5 CreateExperiment failed");
                        Status::internal(format!("create experiment in M5: {e}"))
                    })?;
                resp.into_inner()
            }
            None => {
                // Development / test fallback: mock experiment.
                let mut mock_exp = Experiment {
                    name: format!("Promoted from flag: {}", flag.name),
                    description: format!(
                        "Auto-promoted from feature flag {} ({})",
                        flag.name, flag.flag_id
                    ),
                    owner_email: "system".into(),
                    r#type: exp_type as i32,
                    layer_id: self.default_layer_id.clone(),
                    primary_metric_id: req.primary_metric_id.clone(),
                    secondary_metric_ids: req.secondary_metric_ids.clone(),
                    hash_salt: flag.salt.clone(),
                    experiment_id: Uuid::new_v4().to_string(),
                    state: ExperimentState::Draft as i32,
                    ..Default::default()
                };
                apply_type_config(&mut mock_exp, &flag)?;
                info!(
                    flag_id = %flag.flag_id,
                    experiment_id = %mock_exp.experiment_id,
                    "PromoteToExperiment (mocked — M5_ADDR not configured)"
                );
                mock_exp
            }
        };

        // Link flag → experiment (non-fatal on failure).
        if !result.experiment_id.is_empty() {
            if let Ok(exp_uuid) = Uuid::parse_str(&result.experiment_id) {
                if let Err(e) = self.store.link_flag_to_experiment(flag_id, exp_uuid).await {
                    warn!(
                        error = %e,
                        %flag_id,
                        experiment_id = %result.experiment_id,
                        "link_flag_to_experiment failed (non-fatal)"
                    );
                }
            }
        }

        record_audit_nonfatal(
            &self.audit,
            flag_id,
            "promote_to_experiment",
            "system",
            Some(&flag),
            None,
        )
        .await;

        info!(
            %flag_id,
            experiment_id = %result.experiment_id,
            "PromoteToExperiment succeeded"
        );
        Ok(Response::new(result))
    }
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

pub async fn serve(config: FlagsConfig, store: FlagStore, audit: Option<Arc<AuditStore>>) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    let mut handler = FlagsServiceHandler::new(store, &config);
    if let Some(a) = audit {
        handler = handler.with_audit(a);
    }

    if let Some(m5_addr) = &config.m5_addr {
        match tonic::transport::Channel::from_shared(m5_addr.clone())
            .map_err(|e| format!("invalid M5_ADDR: {e}"))?
            .connect()
            .await
        {
            Ok(channel) => {
                let client = ExperimentManagementServiceClient::new(channel);
                handler = handler.with_m5_client(client);
                info!(%m5_addr, "connected to M5 management service");
            }
            Err(e) => {
                warn!(error = %e, %m5_addr, "M5 connection failed — PromoteToExperiment will use mock");
            }
        }
    }

    let svc = FeatureFlagServiceServer::new(handler);

    info!(%addr, "feature flag gRPC server starting (tonic-web enabled)");

    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(tonic_web::enable(svc))
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}
