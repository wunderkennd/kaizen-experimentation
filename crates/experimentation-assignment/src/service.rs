//! Assignment service implementation.
//!
//! Core logic: deterministic hash-based bucketing using experimentation-hash.
//! Config is read from a live cache backed by `tokio::sync::watch`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rand::rngs::StdRng;
use rand::SeedableRng;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use experimentation_proto::experimentation::assignment::v1::{
    assignment_service_server::AssignmentService, ConfigUpdate, GetAssignmentRequest,
    GetAssignmentResponse, GetAssignmentsRequest, GetAssignmentsResponse,
    GetSlateAssignmentRequest, GetSlateAssignmentResponse,
    GetInterleavedListRequest, GetInterleavedListResponse, RankedList, StreamConfigUpdatesRequest,
};

use crate::bandit_client::{self, GrpcBanditClient};
use crate::config::{Config, ExperimentConfig};
use crate::config_cache::ConfigCacheHandle;
use crate::targeting;

/// gRPC service implementation backed by a live config cache.
pub struct AssignmentServiceImpl {
    config: ConfigCacheHandle,
    bandit_client: Option<GrpcBanditClient>,
}

impl AssignmentServiceImpl {
    pub fn new(config: ConfigCacheHandle, bandit_client: Option<GrpcBanditClient>) -> Self {
        Self {
            config,
            bandit_client,
        }
    }

    /// Wrap a static `Arc<Config>` for tests and backward compatibility.
    ///
    /// Uses no bandit client (uniform random fallback for bandit experiments).
    pub fn from_config(config: Arc<Config>) -> Self {
        Self {
            config: ConfigCacheHandle::from_static(config),
            bandit_client: None,
        }
    }

    /// Get the current config snapshot (for HTTP handler bulk path).
    pub fn config_snapshot(&self) -> Arc<Config> {
        self.config.snapshot()
    }

    /// Core assignment logic.
    ///
    /// Returns `Ok(response)` on success, `Err(Status)` on lookup failure.
    /// Async because bandit experiments may call M4b SelectArm gRPC.
    #[allow(clippy::result_large_err)]
    pub async fn assign(
        &self,
        experiment_id: &str,
        user_id: &str,
        session_id: &str,
        attributes: &HashMap<String, String>,
    ) -> Result<GetAssignmentResponse, Status> {
        let config = self.config.snapshot();

        // 1. Look up experiment.
        let exp = config
            .experiments_by_id
            .get(experiment_id)
            .ok_or_else(|| Status::not_found(format!("experiment not found: {experiment_id}")))?;

        // 2. Check experiment state — only RUNNING serves assignments.
        if exp.state != "RUNNING" {
            return Ok(GetAssignmentResponse {
                experiment_id: experiment_id.to_string(),
                is_active: false,
                ..Default::default()
            });
        }

        // 3. Evaluate targeting rule — user must match to be eligible.
        if let Some(ref rule) = exp.targeting_rule {
            if !targeting::evaluate(rule, attributes) {
                return Ok(GetAssignmentResponse {
                    experiment_id: experiment_id.to_string(),
                    is_active: true,
                    ..Default::default()
                });
            }
        }

        // 4. Get layer total_buckets.
        let layer = config
            .layers_by_id
            .get(&exp.layer_id)
            .ok_or_else(|| Status::internal(format!("layer not found: {}", exp.layer_id)))?;

        // 5. Bandit delegation: MAB / CONTEXTUAL_BANDIT use arm selection, not bucketing.
        if exp.r#type == "MAB" || exp.r#type == "CONTEXTUAL_BANDIT" {
            return self
                .assign_bandit(exp, user_id, experiment_id, attributes)
                .await;
        }

        // 5b. Switchback temporal assignment (ADR-022).
        if exp.r#type == "SWITCHBACK" {
            return self.assign_switchback(exp, experiment_id, attributes);
        }

        // 6. Hash entity into a bucket (user_id for AB, session_id for SESSION_LEVEL).
        //    SESSION_LEVEL with allow_cross_session_variation=false hashes on user_id
        //    to lock variant across sessions. session_id is still required for metrics.
        let hash_input = if exp.r#type == "SESSION_LEVEL" {
            if session_id.is_empty() {
                return Err(Status::invalid_argument(
                    "session_id required for session-level experiment",
                ));
            }
            let cross_session = exp
                .session_config
                .as_ref()
                .is_none_or(|sc| sc.allow_cross_session_variation);
            if cross_session {
                session_id
            } else {
                user_id
            }
        } else {
            user_id
        };
        let bucket = experimentation_hash::bucket(hash_input, &exp.hash_salt, layer.total_buckets);

        // 7. Check allocation range.
        if !experimentation_hash::is_in_allocation(
            bucket,
            exp.allocation.start_bucket,
            exp.allocation.end_bucket,
        ) {
            return Ok(GetAssignmentResponse {
                experiment_id: experiment_id.to_string(),
                is_active: true,
                ..Default::default()
            });
        }

        // 8. Map bucket to variant.
        let variant = select_variant(exp, bucket);

        Ok(GetAssignmentResponse {
            experiment_id: experiment_id.to_string(),
            variant_id: variant.variant_id.clone(),
            payload_json: variant.payload_json.clone(),
            assignment_probability: variant.traffic_fraction,
            is_active: true,
            ..Default::default()
        })
    }

    /// Bandit arm selection for MAB / CONTEXTUAL_BANDIT experiments.
    ///
    /// If a gRPC bandit client is configured, calls M4b `SelectArm` with a 10ms
    /// timeout. On success, the arm payload is looked up from local config (M4b
    /// only selects the arm, not the payload). On timeout or error, falls back
    /// to uniform random selection.
    ///
    /// If no bandit client is configured, always uses uniform random.
    #[allow(clippy::result_large_err)]
    async fn assign_bandit(
        &self,
        exp: &ExperimentConfig,
        user_id: &str,
        experiment_id: &str,
        attributes: &HashMap<String, String>,
    ) -> Result<GetAssignmentResponse, Status> {
        let bandit_config = exp.bandit_config.as_ref().ok_or_else(|| {
            Status::failed_precondition(format!(
                "experiment {experiment_id} is MAB/CONTEXTUAL_BANDIT but has no bandit_config",
            ))
        })?;

        // Try live M4b client first.
        if let Some(ref client) = self.bandit_client {
            let context_features =
                bandit_client::extract_context_features(bandit_config, attributes);

            match client
                .select_arm(experiment_id, user_id, context_features)
                .await
            {
                Ok(result) => {
                    let payload =
                        bandit_client::lookup_arm_payload(&bandit_config.arms, &result.arm_id);
                    return Ok(GetAssignmentResponse {
                        experiment_id: experiment_id.to_string(),
                        variant_id: result.arm_id,
                        payload_json: payload,
                        assignment_probability: result.assignment_probability,
                        is_active: true,
                        ..Default::default()
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        experiment_id,
                        error = %e,
                        "M4b SelectArm failed, falling back to uniform random",
                    );
                    // Fall through to uniform random below.
                }
            }
        }

        // Fallback: deterministic uniform random arm selection.
        let seed_input = format!("{user_id}\x00{experiment_id}");
        let lo = experimentation_hash::murmur3::murmurhash3_x86_32(seed_input.as_bytes(), 0) as u64;
        let hi = experimentation_hash::murmur3::murmurhash3_x86_32(seed_input.as_bytes(), 1) as u64;
        let seed = (hi << 32) | lo;
        let mut rng = StdRng::seed_from_u64(seed);

        let selection =
            bandit_client::select_arm_uniform(bandit_config, &mut rng).ok_or_else(|| {
                Status::failed_precondition(format!(
                    "experiment {experiment_id} bandit_config has no arms",
                ))
            })?;

        Ok(GetAssignmentResponse {
            experiment_id: experiment_id.to_string(),
            variant_id: selection.arm_id,
            payload_json: selection.payload_json,
            assignment_probability: selection.assignment_probability,
            is_active: true,
            ..Default::default()
        })
    }

    /// Switchback temporal assignment for `SWITCHBACK` experiment type (ADR-022).
    ///
    /// Assignment is based on `(current_unix_secs, block_duration, cluster_attribute)`.
    /// Returns an empty `variant_id` when the request falls in a washout window.
    #[allow(clippy::result_large_err)]
    fn assign_switchback(
        &self,
        exp: &crate::config::ExperimentConfig,
        experiment_id: &str,
        attributes: &HashMap<String, String>,
    ) -> Result<GetAssignmentResponse, Status> {
        let sb = exp.switchback_config.as_ref().ok_or_else(|| {
            Status::failed_precondition(format!(
                "experiment {experiment_id} is SWITCHBACK but has no switchback_config",
            ))
        })?;

        // Validate config — mirrors the M5 STARTING-phase gate.
        crate::switchback::validate_config(sb)
            .map_err(Status::failed_precondition)?;

        // Current wall-clock time from the server.
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Washout exclusion: return empty assignment during the washout window.
        if crate::switchback::is_in_washout(
            now_secs,
            sb.block_duration_secs,
            sb.washout_period_secs,
        ) {
            tracing::debug!(
                experiment_id,
                block_duration_secs = sb.block_duration_secs,
                washout_period_secs = sb.washout_period_secs,
                "switchback washout: excluding user",
            );
            return Ok(GetAssignmentResponse {
                experiment_id: experiment_id.to_string(),
                is_active: true,
                ..Default::default()
            });
        }

        let block_index =
            crate::switchback::compute_block_index(now_secs, sb.block_duration_secs);

        let cluster_value = if sb.cluster_attribute.is_empty() {
            String::new()
        } else {
            attributes
                .get(&sb.cluster_attribute)
                .cloned()
                .unwrap_or_default()
        };

        let variant = crate::switchback::select_variant(
            block_index,
            &sb.design,
            &cluster_value,
            experiment_id,
            &exp.variants,
        );

        tracing::debug!(
            experiment_id,
            block_index,
            cluster_value = %cluster_value,
            design = %sb.design,
            variant_id = %variant.variant_id,
            "switchback assignment",
        );

        Ok(GetAssignmentResponse {
            experiment_id: experiment_id.to_string(),
            variant_id: variant.variant_id.clone(),
            payload_json: variant.payload_json.clone(),
            // Switchback assignment is deterministic; probability is 1.0.
            assignment_probability: 1.0,
            is_active: true,
            block_index,
        })
    }

    /// Produce an interleaved list for a given experiment and user.
    ///
    /// Validates config, derives a deterministic RNG seed from (user_id, experiment_id),
    /// then dispatches to the appropriate interleaving algorithm.
    #[allow(clippy::result_large_err)]
    pub fn interleave(
        &self,
        experiment_id: &str,
        user_id: &str,
        algorithm_lists: &HashMap<String, RankedList>,
    ) -> Result<GetInterleavedListResponse, Status> {
        let config = self.config.snapshot();

        // 1. Look up experiment.
        let exp = config
            .experiments_by_id
            .get(experiment_id)
            .ok_or_else(|| Status::not_found(format!("experiment not found: {experiment_id}")))?;

        // 2. Experiment must be RUNNING.
        if exp.state != "RUNNING" {
            return Err(Status::failed_precondition(format!(
                "experiment {experiment_id} is not RUNNING (state: {})",
                exp.state,
            )));
        }

        // 3. Must have interleaving_config.
        let il_config = exp.interleaving_config.as_ref().ok_or_else(|| {
            Status::failed_precondition(format!(
                "experiment {experiment_id} has no interleaving_config",
            ))
        })?;

        // 4. Derive deterministic 64-bit seed from (user_id, experiment_id).
        let seed_input = format!("{user_id}\x00{experiment_id}");
        let lo = experimentation_hash::murmur3::murmurhash3_x86_32(seed_input.as_bytes(), 0) as u64;
        let hi = experimentation_hash::murmur3::murmurhash3_x86_32(seed_input.as_bytes(), 1) as u64;
        let seed = (hi << 32) | lo;
        let mut rng = StdRng::seed_from_u64(seed);

        // 5. Dispatch by method.
        let result = match il_config.method.as_str() {
            "TEAM_DRAFT" | "" => {
                let (algo_a_id, algo_b_id, list_a, list_b) =
                    Self::extract_pairwise(il_config, algorithm_lists)?;
                let k = il_config.max_list_size.min(list_a.len() + list_b.len());
                experimentation_interleaving::team_draft::team_draft(
                    list_a, list_b, algo_a_id, algo_b_id, k, &mut rng,
                )
            }
            "OPTIMIZED" => {
                let (algo_a_id, algo_b_id, list_a, list_b) =
                    Self::extract_pairwise(il_config, algorithm_lists)?;
                let k = il_config.max_list_size.min(list_a.len() + list_b.len());
                experimentation_interleaving::optimized::optimized_interleave(
                    list_a, list_b, algo_a_id, algo_b_id, k, &mut rng,
                )
            }
            "MULTILEAVE" => {
                // Require >= 3 algorithm_ids for multileave.
                if il_config.algorithm_ids.len() < 3 {
                    return Err(Status::failed_precondition(format!(
                        "MULTILEAVE requires >= 3 algorithm_ids, got {}",
                        il_config.algorithm_ids.len(),
                    )));
                }

                // Build ordered (list, algo_id) vec from config order.
                let mut ordered_lists: Vec<(&[String], &str)> = Vec::new();
                let mut total_items = 0usize;
                for algo_id in &il_config.algorithm_ids {
                    let ranked = algorithm_lists.get(algo_id).ok_or_else(|| {
                        Status::invalid_argument(format!("missing algorithm list for '{algo_id}'",))
                    })?;
                    total_items += ranked.item_ids.len();
                    ordered_lists.push((&ranked.item_ids, algo_id.as_str()));
                }

                let k = il_config.max_list_size.min(total_items);
                experimentation_interleaving::multileave::multileave(&ordered_lists, k, &mut rng)
            }
            other => {
                return Err(Status::invalid_argument(format!(
                    "unsupported interleaving method: {other}",
                )));
            }
        };

        Ok(GetInterleavedListResponse {
            merged_list: result.merged_list,
            provenance: result.provenance,
        })
    }

    /// Validate and extract pairwise algorithm lists from a request.
    ///
    /// Enforces exactly 2 algorithm_ids in config and exactly 2 lists in request.
    #[allow(clippy::result_large_err, clippy::type_complexity)]
    fn extract_pairwise<'a>(
        il_config: &'a crate::config::InterleavingConfig,
        algorithm_lists: &'a HashMap<String, RankedList>,
    ) -> Result<(&'a str, &'a str, &'a [String], &'a [String]), Status> {
        if algorithm_lists.len() != 2 {
            return Err(Status::invalid_argument(format!(
                "expected exactly 2 algorithm lists, got {}",
                algorithm_lists.len(),
            )));
        }
        if il_config.algorithm_ids.len() != 2 {
            return Err(Status::failed_precondition(format!(
                "interleaving_config.algorithm_ids must have exactly 2 entries, got {}",
                il_config.algorithm_ids.len(),
            )));
        }
        let algo_a_id = il_config.algorithm_ids[0].as_str();
        let algo_b_id = il_config.algorithm_ids[1].as_str();

        let list_a = algorithm_lists.get(algo_a_id).ok_or_else(|| {
            Status::invalid_argument(format!("missing algorithm list for '{algo_a_id}'"))
        })?;
        let list_b = algorithm_lists.get(algo_b_id).ok_or_else(|| {
            Status::invalid_argument(format!("missing algorithm list for '{algo_b_id}'"))
        })?;

        Ok((algo_a_id, algo_b_id, &list_a.item_ids, &list_b.item_ids))
    }
}

/// Select a variant based on the user's bucket within the allocation range.
///
/// Uses traffic_fraction to partition the allocation range. Falls through to the
/// last variant if floating-point rounding causes no match (total function).
fn select_variant(exp: &ExperimentConfig, bucket: u32) -> &crate::config::VariantConfig {
    let alloc_size = (exp.allocation.end_bucket - exp.allocation.start_bucket + 1) as f64;
    let relative_bucket = (bucket - exp.allocation.start_bucket) as f64;

    let mut cumulative = 0.0_f64;
    for variant in &exp.variants {
        cumulative += variant.traffic_fraction * alloc_size;
        if relative_bucket < cumulative {
            return variant;
        }
    }

    // Fallthrough guard: assign to last variant (handles FP rounding edge cases).
    exp.variants
        .last()
        .expect("experiment must have at least one variant")
}

#[tonic::async_trait]
impl AssignmentService for AssignmentServiceImpl {
    async fn get_assignment(
        &self,
        request: Request<GetAssignmentRequest>,
    ) -> Result<Response<GetAssignmentResponse>, Status> {
        let req = request.into_inner();
        let resp = self
            .assign(
                &req.experiment_id,
                &req.user_id,
                &req.session_id,
                &req.attributes,
            )
            .await?;
        Ok(Response::new(resp))
    }

    async fn get_assignments(
        &self,
        request: Request<GetAssignmentsRequest>,
    ) -> Result<Response<GetAssignmentsResponse>, Status> {
        let req = request.into_inner();
        let config = self.config.snapshot();
        let mut assignments = Vec::new();

        // Two-phase evaluation: holdouts first, then regular experiments.
        // Holdout users are excluded from other experiments in the same layer.
        let (holdouts, regular): (Vec<_>, Vec<_>) = config
            .experiments
            .iter()
            .partition(|e| e.is_cumulative_holdout);

        // Phase 1: Evaluate holdout experiments. Track layers claimed by holdouts.
        // On holdout assignment error, treat the layer as held-out (fail-closed)
        // to avoid accidentally exposing holdout users to treatments.
        let mut holdout_layers: HashSet<String> = HashSet::new();
        for exp in &holdouts {
            match self
                .assign(
                    &exp.experiment_id,
                    &req.user_id,
                    &req.session_id,
                    &req.attributes,
                )
                .await
            {
                Ok(resp) => {
                    if !resp.variant_id.is_empty() {
                        holdout_layers.insert(exp.layer_id.clone());
                    }
                    assignments.push(resp);
                }
                Err(e) => {
                    // Fail-closed: mark this layer as held-out so that the user
                    // is NOT assigned to regular experiments in this layer.
                    // This prevents holdout leakage when the holdout assignment
                    // itself fails (e.g., missing layer config).
                    tracing::warn!(
                        experiment_id = %exp.experiment_id,
                        layer_id = %exp.layer_id,
                        error = %e,
                        "holdout assignment failed, excluding layer (fail-closed)",
                    );
                    holdout_layers.insert(exp.layer_id.clone());
                }
            }
        }

        // Phase 2: Evaluate regular experiments, skipping layers claimed by holdouts.
        for exp in &regular {
            if holdout_layers.contains(&exp.layer_id) {
                continue;
            }
            match self
                .assign(
                    &exp.experiment_id,
                    &req.user_id,
                    &req.session_id,
                    &req.attributes,
                )
                .await
            {
                Ok(resp) => {
                    assignments.push(resp);
                }
                Err(e) => {
                    tracing::warn!(
                        experiment_id = %exp.experiment_id,
                        error = %e,
                        "assignment failed for regular experiment, skipping",
                    );
                }
            }
        }

        Ok(Response::new(GetAssignmentsResponse { assignments }))
    }

    async fn get_interleaved_list(
        &self,
        request: Request<GetInterleavedListRequest>,
    ) -> Result<Response<GetInterleavedListResponse>, Status> {
        let req = request.into_inner();
        let resp = self.interleave(&req.experiment_id, &req.user_id, &req.algorithm_lists)?;
        Ok(Response::new(resp))
    }

    type StreamConfigUpdatesStream = ReceiverStream<Result<ConfigUpdate, Status>>;

    async fn stream_config_updates(
        &self,
        _request: Request<StreamConfigUpdatesRequest>,
    ) -> Result<Response<Self::StreamConfigUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "StreamConfigUpdates not yet implemented (M5 integration)",
        ))
    }

    async fn get_slate_assignment(
        &self,
        _request: Request<GetSlateAssignmentRequest>,
    ) -> Result<Response<GetSlateAssignmentResponse>, Status> {
        Err(Status::unimplemented(
            "GetSlateAssignment not yet implemented (ADR-016)",
        ))
    }
}
