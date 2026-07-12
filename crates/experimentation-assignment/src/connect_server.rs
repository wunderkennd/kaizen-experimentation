//! ADR-031 pilot — ConnectRPC server adapter for M1 `AssignmentService`.
//!
//! Bridges buffa request/response views to the existing
//! [`AssignmentServiceImpl`] domain methods. #641 shipped `GetAssignment`
//! end-to-end; #642 extends the same bridge to `GetAssignments`,
//! `GetSlateAssignment`, and `GetInterleavedList` (the three remaining unary
//! RPCs). `StreamConfigUpdates` (server-streaming) lands in #643.
//!
//! Every bridge is a thin field-copy delegating to a pub domain method on
//! `AssignmentServiceImpl` — the `assert_finite!` invariants live inside
//! those domain methods and pass through untouched.

use std::collections::HashMap;
use std::sync::Arc;

use experimentation_proto::experimentation::assignment::v1 as domain_pb;
use experimentation_proto_connect::experimentation::assignment::v1 as connect_pb;

use crate::service::AssignmentServiceImpl;

pub struct ConnectAssignment {
    inner: Arc<AssignmentServiceImpl>,
}

impl ConnectAssignment {
    pub fn new(inner: Arc<AssignmentServiceImpl>) -> Self {
        Self { inner }
    }
}

fn tonic_status_to_connect(status: tonic::Status) -> connectrpc::ConnectError {
    let code = match status.code() {
        tonic::Code::NotFound => connectrpc::ErrorCode::NotFound,
        tonic::Code::InvalidArgument => connectrpc::ErrorCode::InvalidArgument,
        tonic::Code::Unauthenticated => connectrpc::ErrorCode::Unauthenticated,
        tonic::Code::PermissionDenied => connectrpc::ErrorCode::PermissionDenied,
        tonic::Code::ResourceExhausted => connectrpc::ErrorCode::ResourceExhausted,
        tonic::Code::FailedPrecondition => connectrpc::ErrorCode::FailedPrecondition,
        tonic::Code::Unavailable => connectrpc::ErrorCode::Unavailable,
        // tonic spells it "Cancelled"; the Connect protocol spells it "Canceled".
        tonic::Code::Cancelled => connectrpc::ErrorCode::Canceled,
        tonic::Code::DeadlineExceeded => connectrpc::ErrorCode::DeadlineExceeded,
        _ => connectrpc::ErrorCode::Internal,
    };
    connectrpc::ConnectError::new(code, status.message().to_string())
}

fn unimplemented(msg: &'static str) -> connectrpc::ConnectError {
    connectrpc::ConnectError::new(connectrpc::ErrorCode::Unimplemented, msg)
}

/// Field-copy from the tonic-side domain response to the buffa response.
/// Called from both the single-assignment and batch (`GetAssignments`) paths,
/// so any future field addition to the response only needs to touch one
/// place. `assert_finite!` is enforced inside `AssignmentServiceImpl::assign`
/// on `assignment_probability`; here we faithfully pass the value through.
fn assignment_domain_to_connect(
    d: domain_pb::GetAssignmentResponse,
) -> connect_pb::GetAssignmentResponse {
    connect_pb::GetAssignmentResponse {
        experiment_id: d.experiment_id,
        variant_id: d.variant_id,
        payload_json: d.payload_json,
        assignment_probability: d.assignment_probability,
        is_active: d.is_active,
        // Switchback experiments compute a non-zero time-block index that M4a
        // needs for within-block vs cross-block analysis; must not default to 0.
        block_index: d.block_index,
        ..Default::default()
    }
}

#[allow(refining_impl_trait)]
impl connect_pb::AssignmentService for ConnectAssignment {
    async fn get_assignment(
        &self,
        _ctx: connectrpc::RequestContext,
        request: connectrpc::ServiceRequest<'_, connect_pb::GetAssignmentRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetAssignmentResponse> {
        let experiment_id = request.experiment_id.to_string();
        let user_id = request.user_id.to_string();
        let session_id = request.session_id.to_string();
        let attributes: HashMap<String, String> = request
            .attributes
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();

        let resp = self
            .inner
            .assign(&experiment_id, &user_id, &session_id, &attributes)
            .await
            .map_err(tonic_status_to_connect)?;

        Ok(connectrpc::Response::new(assignment_domain_to_connect(resp)))
    }

    async fn get_assignments(
        &self,
        _ctx: connectrpc::RequestContext,
        request: connectrpc::ServiceRequest<'_, connect_pb::GetAssignmentsRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetAssignmentsResponse> {
        let user_id = request.user_id.to_string();
        let session_id = request.session_id.to_string();
        let attributes: HashMap<String, String> = request
            .attributes
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();

        // Batch path — shares two-phase holdout/layer logic with the tonic
        // handler via AssignmentServiceImpl::assign_batch. Errors are already
        // absorbed inside assign_batch (soft-fail on regular, fail-closed on
        // holdout), so the Connect surface always returns 200.
        let domain = self
            .inner
            .assign_batch(&user_id, &session_id, &attributes)
            .await;

        Ok(connectrpc::Response::new(connect_pb::GetAssignmentsResponse {
            assignments: domain
                .into_iter()
                .map(assignment_domain_to_connect)
                .collect(),
            ..Default::default()
        }))
    }

    async fn get_interleaved_list(
        &self,
        _ctx: connectrpc::RequestContext,
        request: connectrpc::ServiceRequest<'_, connect_pb::GetInterleavedListRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetInterleavedListResponse> {
        let experiment_id = request.experiment_id.to_string();
        let user_id = request.user_id.to_string();

        // buffa's map<string, RankedList> yields borrowed views; interleave
        // wants owned strings so the deterministic seed hashing over
        // (user_id, experiment_id) inside interleave stays stable regardless
        // of transport.
        let algorithm_lists: HashMap<String, domain_pb::RankedList> = request
            .algorithm_lists
            .iter()
            .map(|(algo_id, list)| {
                (
                    (*algo_id).to_string(),
                    domain_pb::RankedList {
                        item_ids: list.item_ids.iter().map(|s| (*s).to_string()).collect(),
                    },
                )
            })
            .collect();

        let resp = self
            .inner
            .interleave(&experiment_id, &user_id, &algorithm_lists)
            .map_err(tonic_status_to_connect)?;

        Ok(connectrpc::Response::new(connect_pb::GetInterleavedListResponse {
            merged_list: resp.merged_list,
            provenance: resp.provenance,
            ..Default::default()
        }))
    }

    async fn stream_config_updates(
        &self,
        _ctx: connectrpc::RequestContext,
        _request: connectrpc::ServiceRequest<'_, connect_pb::StreamConfigUpdatesRequest>,
    ) -> connectrpc::ServiceResult<
        connectrpc::ServiceStream<connect_pb::ConfigUpdate>,
    > {
        Err(unimplemented("ADR-031 pilot: StreamConfigUpdates lands in #643"))
    }

    async fn get_slate_assignment(
        &self,
        _ctx: connectrpc::RequestContext,
        request: connectrpc::ServiceRequest<'_, connect_pb::GetSlateAssignmentRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetSlateAssignmentResponse> {
        let experiment_id = request.experiment_id.to_string();
        let user_id = request.user_id.to_string();
        let candidate_item_ids: Vec<String> = request
            .candidate_item_ids
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let attributes: HashMap<String, String> = request
            .attributes
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();

        let resp = self
            .inner
            .assign_slate(&experiment_id, &user_id, candidate_item_ids, &attributes)
            .await
            .map_err(tonic_status_to_connect)?;

        // slot_probabilities.probability is the IPW field; assert_finite! is
        // enforced inside assign_slate, we forward the domain values verbatim.
        Ok(connectrpc::Response::new(connect_pb::GetSlateAssignmentResponse {
            experiment_id: resp.experiment_id,
            slate_item_ids: resp.slate_item_ids,
            slot_probabilities: resp
                .slot_probabilities
                .into_iter()
                .map(|sp| connect_pb::SlotProbability {
                    slot_index: sp.slot_index,
                    item_id: sp.item_id,
                    probability: sp.probability,
                    ..Default::default()
                })
                .collect(),
            is_uniform_random: resp.is_uniform_random,
            ..Default::default()
        }))
    }
}
