//! ADR-031 pilot — ConnectRPC server adapter for M1 `AssignmentService`.
//!
//! Bridges buffa request/response views to the existing
//! [`AssignmentServiceImpl`] domain methods. #641 shipped `GetAssignment`
//! end-to-end; #642 extends the same bridge to `GetAssignments`,
//! `GetSlateAssignment`, and `GetInterleavedList` (the three remaining unary
//! RPCs). `StreamConfigUpdates` (server-streaming) is wired in this PR (#643).
//!
//! Every bridge is a thin field-copy delegating to a pub domain method on
//! `AssignmentServiceImpl` — the `assert_finite!` invariants live inside
//! those domain methods and pass through untouched.

use std::collections::HashMap;
use std::sync::Arc;

use experimentation_proto::experimentation::assignment::v1 as domain_pb;
use experimentation_proto_connect::experimentation::assignment::v1 as connect_pb;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::StreamExt;

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

/// Bridge one prost-side `ConfigUpdate` to its buffa counterpart. The nested
/// `Experiment` message is a large tree that lives entirely on the prost/tonic
/// side today (56 files, 225 sites — ADR-031 §"central cost"); bridging its
/// full field set requires prost↔buffa parity generation that is out of the
/// pilot's scope. Until M5 integration lands, the pilot streams the two
/// scalar fields (`is_deletion`, `version`) which is sufficient to validate
/// ordering, backpressure, and clean-shutdown parity across transports.
fn config_update_domain_to_connect(
    d: domain_pb::ConfigUpdate,
) -> connect_pb::ConfigUpdate {
    // `experiment` (a MessageField wrapping the nested Experiment) is left
    // at its default (unset) via `..Default::default()` — see the doc
    // comment above for why the nested tree is intentionally omitted.
    connect_pb::ConfigUpdate {
        is_deletion: d.is_deletion,
        version: d.version,
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
        // ADR-031 #643 — Connect bridge for server-streaming. Subscribes to
        // the exact same broadcast source the tonic handler uses so every
        // subscriber, regardless of transport, sees identical events in the
        // same order. `last_known_version` is ignored until M5 integration
        // adds replay; each subscriber starts empty on connect.
        let rx = self.inner.subscribe_config_updates();
        let stream = BroadcastStream::new(rx).map(|r| match r {
            Ok(update) => Ok(config_update_domain_to_connect(update)),
            Err(BroadcastStreamRecvError::Lagged(n)) => Err(
                connectrpc::ConnectError::new(
                    connectrpc::ErrorCode::DataLoss,
                    format!(
                        "config update stream lagged, skipped {n} messages — reconnect with last_known_version",
                    ),
                ),
            ),
        });
        Ok(connectrpc::Response::new(Box::pin(stream)))
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
