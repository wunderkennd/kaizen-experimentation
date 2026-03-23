//! tonic gRPC service implementation for FeatureFlagService (M7 Rust port).
//!
//! Phase 1: CRUD + EvaluateFlag/EvaluateFlags.
//! PromoteToExperiment is stubbed — returns UNIMPLEMENTED (Phase 2).

use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

use experimentation_proto::experimentation::flags::v1::{
    feature_flag_service_server::{FeatureFlagService, FeatureFlagServiceServer},
    CreateFlagRequest, EvaluateFlagRequest, EvaluateFlagResponse, EvaluateFlagsRequest,
    EvaluateFlagsResponse, Flag as ProtoFlag, FlagType, FlagVariant as ProtoFlagVariant,
    GetFlagRequest, ListFlagsRequest, ListFlagsResponse, PromoteToExperimentRequest,
    UpdateFlagRequest,
};
use experimentation_proto::experimentation::common::v1::Experiment;

use crate::config::FlagsConfig;
use crate::store::{Flag, FlagStore, FlagVariant, StoreError};

// ---------------------------------------------------------------------------
// Service handler
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FlagsServiceHandler {
    store: Arc<FlagStore>,
}

impl FlagsServiceHandler {
    pub fn new(store: FlagStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }
}

// ---------------------------------------------------------------------------
// Proto ↔ domain conversions
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
            variant_id: Uuid::nil(), // assigned by DB on insert
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
        salt: String::new(), // assigned by DB on insert
        targeting_rule_id,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        promoted_experiment_id: None,
        promoted_at: None,
        resolved_at: None,
        variants,
    })
}

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

    // User is in rollout.
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

        let domain = proto_to_domain(&pb)?;
        let updated = self
            .store
            .update_flag(&domain)
            .await
            .map_err(store_err_to_status)?;

        info!(flag_id = %updated.flag_id, "flag updated");
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

    /// Phase 2: stub — M5 gRPC client not yet wired.
    async fn promote_to_experiment(
        &self,
        _request: Request<PromoteToExperimentRequest>,
    ) -> Result<Response<Experiment>, Status> {
        Err(Status::unimplemented(
            "PromoteToExperiment: Phase 2 — not yet implemented in Rust port",
        ))
    }
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

pub async fn serve(config: FlagsConfig, store: FlagStore) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    let handler = FlagsServiceHandler::new(store);
    let svc = FeatureFlagServiceServer::new(handler);

    info!(%addr, "feature flag gRPC server starting (tonic-web enabled)");

    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(tonic_web::enable(svc))
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}
