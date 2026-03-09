//! Assignment service implementation.
//!
//! Core logic: deterministic hash-based bucketing using experimentation-hash.
//! Config is read from a live cache backed by `tokio::sync::watch`.

use std::collections::HashMap;
use std::sync::Arc;

use rand::rngs::StdRng;
use rand::SeedableRng;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use experimentation_proto::experimentation::assignment::v1::{
    assignment_service_server::AssignmentService, ConfigUpdate, GetAssignmentRequest,
    GetAssignmentResponse, GetAssignmentsRequest, GetAssignmentsResponse,
    GetInterleavedListRequest, GetInterleavedListResponse, RankedList,
    StreamConfigUpdatesRequest,
};

use crate::config::{Config, ExperimentConfig};
use crate::config_cache::ConfigCacheHandle;
use crate::targeting;

/// gRPC service implementation backed by a live config cache.
pub struct AssignmentServiceImpl {
    config: ConfigCacheHandle,
}

impl AssignmentServiceImpl {
    pub fn new(config: ConfigCacheHandle) -> Self {
        Self { config }
    }

    /// Wrap a static `Arc<Config>` for tests and backward compatibility.
    pub fn from_config(config: Arc<Config>) -> Self {
        Self {
            config: ConfigCacheHandle::from_static(config),
        }
    }

    /// Core assignment logic — pure CPU, no async needed.
    ///
    /// Returns `Ok(response)` on success, `Err(Status)` on lookup failure.
    #[allow(clippy::result_large_err)]
    pub fn assign(
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
            .ok_or_else(|| {
                Status::not_found(format!("experiment not found: {experiment_id}"))
            })?;

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
            .ok_or_else(|| {
                Status::internal(format!("layer not found: {}", exp.layer_id))
            })?;

        // 5. Bandit delegation: MAB / CONTEXTUAL_BANDIT use arm selection, not bucketing.
        if exp.r#type == "MAB" || exp.r#type == "CONTEXTUAL_BANDIT" {
            return self.assign_bandit(exp, user_id, experiment_id);
        }

        // 6. Hash entity into a bucket (user_id for AB, session_id for SESSION_LEVEL).
        let hash_input = if exp.r#type == "SESSION_LEVEL" {
            if session_id.is_empty() {
                return Err(Status::invalid_argument(
                    "session_id required for session-level experiment",
                ));
            }
            session_id
        } else {
            user_id
        };
        let bucket =
            experimentation_hash::bucket(hash_input, &exp.hash_salt, layer.total_buckets);

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
        })
    }

    /// Bandit arm selection for MAB / CONTEXTUAL_BANDIT experiments.
    ///
    /// Derives a deterministic RNG seed from (user_id, experiment_id),
    /// then delegates to the bandit client (currently mock uniform random).
    /// When M4b is live, this will call `SelectArm` gRPC instead.
    #[allow(clippy::result_large_err)]
    fn assign_bandit(
        &self,
        exp: &ExperimentConfig,
        user_id: &str,
        experiment_id: &str,
    ) -> Result<GetAssignmentResponse, Status> {
        let bandit_config = exp.bandit_config.as_ref().ok_or_else(|| {
            Status::failed_precondition(format!(
                "experiment {experiment_id} is MAB/CONTEXTUAL_BANDIT but has no bandit_config",
            ))
        })?;

        // Deterministic seed from (user_id, experiment_id).
        let seed_input = format!("{user_id}\x00{experiment_id}");
        let lo = experimentation_hash::murmur3::murmurhash3_x86_32(
            seed_input.as_bytes(),
            0,
        ) as u64;
        let hi = experimentation_hash::murmur3::murmurhash3_x86_32(
            seed_input.as_bytes(),
            1,
        ) as u64;
        let seed = (hi << 32) | lo;
        let mut rng = StdRng::seed_from_u64(seed);

        // TODO(m4b): Replace with gRPC call to M4b SelectArm when available.
        let selection = crate::bandit_client::select_arm_uniform(bandit_config, &mut rng)
            .ok_or_else(|| {
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
        })
    }

    /// Produce an interleaved list for a given experiment and user.
    ///
    /// Validates config, derives a deterministic RNG seed from (user_id, experiment_id),
    /// then delegates to the Team Draft algorithm.
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
            .ok_or_else(|| {
                Status::not_found(format!("experiment not found: {experiment_id}"))
            })?;

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

        // 4. Request must contain exactly 2 algorithm lists (pairwise interleaving).
        if algorithm_lists.len() != 2 {
            return Err(Status::invalid_argument(format!(
                "expected exactly 2 algorithm lists, got {}",
                algorithm_lists.len(),
            )));
        }

        // 5. Extract lists ordered by config algorithm_ids.
        if il_config.algorithm_ids.len() != 2 {
            return Err(Status::failed_precondition(format!(
                "interleaving_config.algorithm_ids must have exactly 2 entries, got {}",
                il_config.algorithm_ids.len(),
            )));
        }
        let algo_a_id = &il_config.algorithm_ids[0];
        let algo_b_id = &il_config.algorithm_ids[1];

        let list_a = algorithm_lists.get(algo_a_id).ok_or_else(|| {
            Status::invalid_argument(format!(
                "missing algorithm list for '{algo_a_id}'",
            ))
        })?;
        let list_b = algorithm_lists.get(algo_b_id).ok_or_else(|| {
            Status::invalid_argument(format!(
                "missing algorithm list for '{algo_b_id}'",
            ))
        })?;

        // 6. Derive deterministic 64-bit seed from (user_id, experiment_id).
        let seed_input = format!("{user_id}\x00{experiment_id}");
        let lo = experimentation_hash::murmur3::murmurhash3_x86_32(
            seed_input.as_bytes(),
            0,
        ) as u64;
        let hi = experimentation_hash::murmur3::murmurhash3_x86_32(
            seed_input.as_bytes(),
            1,
        ) as u64;
        let seed = (hi << 32) | lo;
        let mut rng = StdRng::seed_from_u64(seed);

        // 7. Compute k = min(max_list_size, total available items).
        let k = il_config
            .max_list_size
            .min(list_a.item_ids.len() + list_b.item_ids.len());

        // 8. Delegate to interleaving algorithm based on method.
        let result = match il_config.method.as_str() {
            "TEAM_DRAFT" | "" => experimentation_interleaving::team_draft::team_draft(
                &list_a.item_ids,
                &list_b.item_ids,
                algo_a_id,
                algo_b_id,
                k,
                &mut rng,
            ),
            "OPTIMIZED" => experimentation_interleaving::optimized::optimized_interleave(
                &list_a.item_ids,
                &list_b.item_ids,
                algo_a_id,
                algo_b_id,
                k,
                &mut rng,
            ),
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
}

/// Select a variant based on the user's bucket within the allocation range.
///
/// Uses traffic_fraction to partition the allocation range. Falls through to the
/// last variant if floating-point rounding causes no match (total function).
fn select_variant(
    exp: &ExperimentConfig,
    bucket: u32,
) -> &crate::config::VariantConfig {
    let alloc_size =
        (exp.allocation.end_bucket - exp.allocation.start_bucket + 1) as f64;
    let relative_bucket = (bucket - exp.allocation.start_bucket) as f64;

    let mut cumulative = 0.0_f64;
    for variant in &exp.variants {
        cumulative += variant.traffic_fraction * alloc_size;
        if relative_bucket < cumulative {
            return variant;
        }
    }

    // Fallthrough guard: assign to last variant (handles FP rounding edge cases).
    exp.variants.last().expect("experiment must have at least one variant")
}

#[tonic::async_trait]
impl AssignmentService for AssignmentServiceImpl {
    async fn get_assignment(
        &self,
        request: Request<GetAssignmentRequest>,
    ) -> Result<Response<GetAssignmentResponse>, Status> {
        let req = request.into_inner();
        let resp = self.assign(&req.experiment_id, &req.user_id, &req.session_id, &req.attributes)?;
        Ok(Response::new(resp))
    }

    async fn get_assignments(
        &self,
        request: Request<GetAssignmentsRequest>,
    ) -> Result<Response<GetAssignmentsResponse>, Status> {
        let req = request.into_inner();
        let config = self.config.snapshot();
        let mut assignments = Vec::new();

        for exp in &config.experiments {
            // Best-effort: skip experiments that fail assignment.
            if let Ok(resp) = self.assign(&exp.experiment_id, &req.user_id, &req.session_id, &req.attributes) {
                assignments.push(resp);
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

    type StreamConfigUpdatesStream =
        ReceiverStream<Result<ConfigUpdate, Status>>;

    async fn stream_config_updates(
        &self,
        _request: Request<StreamConfigUpdatesRequest>,
    ) -> Result<Response<Self::StreamConfigUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "StreamConfigUpdates not yet implemented (M5 integration)",
        ))
    }
}
