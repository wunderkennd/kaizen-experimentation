//! ADR-031 pilot — ConnectRPC server adapter for M1 `AssignmentService`.
//!
//! Bridges buffa request/response views to the existing
//! [`AssignmentServiceImpl`] domain methods. The pilot scope (#641) ships only
//! `GetAssignment` end-to-end; the remaining four RPCs return
//! `Unimplemented` and are filled in by #642/#643.

use std::collections::HashMap;
use std::sync::Arc;

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
        _ => connectrpc::ErrorCode::Internal,
    };
    connectrpc::ConnectError::new(code, status.message().to_string())
}

fn unimplemented(msg: &'static str) -> connectrpc::ConnectError {
    connectrpc::ConnectError::new(connectrpc::ErrorCode::Unimplemented, msg)
}

#[allow(refining_impl_trait)]
impl connect_pb::AssignmentService for ConnectAssignment {
    async fn get_assignment<'a>(
        &'a self,
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

        // assert_finite! is enforced inside AssignmentServiceImpl::assign;
        // here we faithfully pass the value through.
        Ok(connectrpc::Response::new(connect_pb::GetAssignmentResponse {
            experiment_id: resp.experiment_id,
            variant_id: resp.variant_id,
            payload_json: resp.payload_json,
            assignment_probability: resp.assignment_probability,
            is_active: resp.is_active,
            ..Default::default()
        }))
    }

    async fn get_assignments<'a>(
        &'a self,
        _ctx: connectrpc::RequestContext,
        _request: connectrpc::ServiceRequest<'_, connect_pb::GetAssignmentsRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetAssignmentsResponse> {
        Err(unimplemented("ADR-031 pilot: GetAssignments lands in #642"))
    }

    async fn get_interleaved_list<'a>(
        &'a self,
        _ctx: connectrpc::RequestContext,
        _request: connectrpc::ServiceRequest<'_, connect_pb::GetInterleavedListRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetInterleavedListResponse> {
        Err(unimplemented("ADR-031 pilot: GetInterleavedList lands in #642"))
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

    async fn get_slate_assignment<'a>(
        &'a self,
        _ctx: connectrpc::RequestContext,
        _request: connectrpc::ServiceRequest<'_, connect_pb::GetSlateAssignmentRequest>,
    ) -> connectrpc::ServiceResult<connect_pb::GetSlateAssignmentResponse> {
        Err(unimplemented("ADR-031 pilot: GetSlateAssignment lands in #642"))
    }
}
