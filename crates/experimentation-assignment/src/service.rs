//! Assignment service implementation.
//!
//! Core logic: deterministic hash-based bucketing using experimentation-hash.
//! Config is loaded once at startup as `Arc<Config>` (read-only, no locks).

use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use experimentation_proto::experimentation::assignment::v1::{
    assignment_service_server::AssignmentService, ConfigUpdate, GetAssignmentRequest,
    GetAssignmentResponse, GetAssignmentsRequest, GetAssignmentsResponse,
    GetInterleavedListRequest, GetInterleavedListResponse, StreamConfigUpdatesRequest,
};

use crate::config::{Config, ExperimentConfig};

/// gRPC service implementation backed by a static config snapshot.
pub struct AssignmentServiceImpl {
    config: Arc<Config>,
}

impl AssignmentServiceImpl {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Core assignment logic — pure CPU, no async needed.
    ///
    /// Returns `Ok(response)` on success, `Err(Status)` on lookup failure.
    pub fn assign(
        &self,
        experiment_id: &str,
        user_id: &str,
    ) -> Result<GetAssignmentResponse, Status> {
        // 1. Look up experiment.
        let exp = self
            .config
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

        // 3. Get layer total_buckets.
        let layer = self
            .config
            .layers_by_id
            .get(&exp.layer_id)
            .ok_or_else(|| {
                Status::internal(format!("layer not found: {}", exp.layer_id))
            })?;

        // 4. Hash user into a bucket.
        let bucket =
            experimentation_hash::bucket(user_id, &exp.hash_salt, layer.total_buckets);

        // 5. Check allocation range.
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

        // 6. Map bucket to variant.
        let variant = select_variant(exp, bucket);

        Ok(GetAssignmentResponse {
            experiment_id: experiment_id.to_string(),
            variant_id: variant.variant_id.clone(),
            payload_json: variant.payload_json.clone(),
            assignment_probability: variant.traffic_fraction,
            is_active: true,
        })
    }
}

/// Select a variant based on the user's bucket within the allocation range.
///
/// Uses traffic_fraction to partition the allocation range. Falls through to the
/// last variant if floating-point rounding causes no match (total function).
fn select_variant<'a>(
    exp: &'a ExperimentConfig,
    bucket: u32,
) -> &'a crate::config::VariantConfig {
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
        let resp = self.assign(&req.experiment_id, &req.user_id)?;
        Ok(Response::new(resp))
    }

    async fn get_assignments(
        &self,
        request: Request<GetAssignmentsRequest>,
    ) -> Result<Response<GetAssignmentsResponse>, Status> {
        let req = request.into_inner();
        let mut assignments = Vec::new();

        for exp in &self.config.experiments {
            // Best-effort: skip experiments that fail assignment.
            if let Ok(resp) = self.assign(&exp.experiment_id, &req.user_id) {
                assignments.push(resp);
            }
        }

        Ok(Response::new(GetAssignmentsResponse { assignments }))
    }

    async fn get_interleaved_list(
        &self,
        _request: Request<GetInterleavedListRequest>,
    ) -> Result<Response<GetInterleavedListResponse>, Status> {
        Err(Status::unimplemented(
            "GetInterleavedList not yet implemented (Phase 2)",
        ))
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
