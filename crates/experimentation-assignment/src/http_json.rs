//! JSON HTTP API for the Assignment Service.
//!
//! Provides a lightweight HTTP+JSON interface compatible with ConnectRPC unary
//! protocol conventions. SDKs use this instead of raw gRPC for simplicity.
//!
//! Routes:
//!   POST /experimentation.assignment.v1.AssignmentService/GetAssignment
//!   POST /experimentation.assignment.v1.AssignmentService/GetAssignments

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::service::AssignmentServiceImpl;

// ---------------------------------------------------------------------------
// JSON request/response types (matching proto JSON encoding)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetAssignmentJsonRequest {
    user_id: String,
    experiment_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GetAssignmentJsonResponse {
    experiment_id: String,
    variant_id: String,
    payload_json: String,
    assignment_probability: f64,
    is_active: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetAssignmentsJsonRequest {
    user_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GetAssignmentsJsonResponse {
    assignments: Vec<GetAssignmentJsonResponse>,
}

// ---------------------------------------------------------------------------
// HTTP handler
// ---------------------------------------------------------------------------

const GET_ASSIGNMENT_PATH: &str =
    "/experimentation.assignment.v1.AssignmentService/GetAssignment";
const GET_ASSIGNMENTS_PATH: &str =
    "/experimentation.assignment.v1.AssignmentService/GetAssignments";

type BoxBody = Full<Bytes>;

fn json_response(status: StatusCode, body: &[u8]) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-methods", "POST, OPTIONS")
        .header("access-control-allow-headers", "content-type")
        .body(Full::new(Bytes::copy_from_slice(body)))
        .expect("response builder should not fail")
}

fn error_json(status: StatusCode, message: &str) -> Response<BoxBody> {
    let body = serde_json::json!({ "code": status.as_u16(), "message": message });
    json_response(status, body.to_string().as_bytes())
}

async fn handle_request(
    svc: Arc<AssignmentServiceImpl>,
    req: Request<Incoming>,
) -> Result<Response<BoxBody>, hyper::Error> {
    // CORS preflight
    if req.method() == Method::OPTIONS {
        return Ok(json_response(StatusCode::NO_CONTENT, b""));
    }

    if req.method() != Method::POST {
        return Ok(error_json(
            StatusCode::METHOD_NOT_ALLOWED,
            "only POST is supported",
        ));
    }

    let path = req.uri().path().to_string();
    let body_bytes = req.collect().await?.to_bytes();

    match path.as_str() {
        GET_ASSIGNMENT_PATH => handle_get_assignment(&svc, &body_bytes).await,
        GET_ASSIGNMENTS_PATH => handle_get_assignments(&svc, &body_bytes).await,
        _ => Ok(error_json(StatusCode::NOT_FOUND, "unknown method")),
    }
}

async fn handle_get_assignment(
    svc: &AssignmentServiceImpl,
    body: &[u8],
) -> Result<Response<BoxBody>, hyper::Error> {
    let req: GetAssignmentJsonRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return Ok(error_json(StatusCode::BAD_REQUEST, &e.to_string())),
    };

    match svc
        .assign(&req.experiment_id, &req.user_id, &req.session_id, &req.attributes)
        .await
    {
        Ok(resp) => {
            let json_resp = GetAssignmentJsonResponse {
                experiment_id: resp.experiment_id,
                variant_id: resp.variant_id,
                payload_json: resp.payload_json,
                assignment_probability: resp.assignment_probability,
                is_active: resp.is_active,
            };
            let body = serde_json::to_vec(&json_resp).expect("serialize should not fail");
            Ok(json_response(StatusCode::OK, &body))
        }
        Err(status) => {
            let http_status = match status.code() {
                tonic::Code::NotFound => StatusCode::NOT_FOUND,
                tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Ok(error_json(http_status, status.message()))
        }
    }
}

async fn handle_get_assignments(
    svc: &AssignmentServiceImpl,
    body: &[u8],
) -> Result<Response<BoxBody>, hyper::Error> {
    let req: GetAssignmentsJsonRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return Ok(error_json(StatusCode::BAD_REQUEST, &e.to_string())),
    };

    // Iterate all experiments and call assign() for each one (mirrors gRPC bulk path).
    let config = svc.config_snapshot();
    let mut assignments = Vec::new();

    for exp in &config.experiments {
        match svc
            .assign(&exp.experiment_id, &req.user_id, &req.session_id, &req.attributes)
            .await
        {
            Ok(resp) => {
                assignments.push(GetAssignmentJsonResponse {
                    experiment_id: resp.experiment_id,
                    variant_id: resp.variant_id,
                    payload_json: resp.payload_json,
                    assignment_probability: resp.assignment_probability,
                    is_active: resp.is_active,
                });
            }
            Err(_) => continue,
        }
    }

    let json_resp = GetAssignmentsJsonResponse { assignments };
    let body = serde_json::to_vec(&json_resp).expect("serialize should not fail");
    Ok(json_response(StatusCode::OK, &body))
}

/// Start the JSON HTTP server on the given address.
///
/// This runs alongside the gRPC server and shares the same `AssignmentServiceImpl`.
pub async fn serve(addr: SocketAddr, svc: Arc<AssignmentServiceImpl>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "JSON HTTP server listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let svc = svc.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let svc = svc.clone();
            if let Err(e) = http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req| {
                        let svc = svc.clone();
                        handle_request(svc, req)
                    }),
                )
                .await
            {
                tracing::warn!(error = %e, "HTTP connection error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_get_assignment_request() {
        let json = r#"{"userId":"u1","experimentId":"exp1"}"#;
        let req: GetAssignmentJsonRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.user_id, "u1");
        assert_eq!(req.experiment_id, "exp1");
        assert!(req.session_id.is_empty());
        assert!(req.attributes.is_empty());
    }

    #[test]
    fn test_deserialize_with_attributes() {
        let json = r#"{"userId":"u1","experimentId":"exp1","attributes":{"plan":"premium"}}"#;
        let req: GetAssignmentJsonRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.attributes.get("plan").unwrap(), "premium");
    }

    #[test]
    fn test_serialize_response() {
        let resp = GetAssignmentJsonResponse {
            experiment_id: "exp1".into(),
            variant_id: "control".into(),
            payload_json: "{}".into(),
            assignment_probability: 0.5,
            is_active: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"experimentId\":\"exp1\""));
        assert!(json.contains("\"variantId\":\"control\""));
        assert!(json.contains("\"isActive\":true"));
    }

    #[test]
    fn test_deserialize_get_assignments_request() {
        let json = r#"{"userId":"u1"}"#;
        let req: GetAssignmentsJsonRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.user_id, "u1");
        assert!(req.session_id.is_empty());
    }

    #[test]
    fn test_error_json_format() {
        let resp = error_json(StatusCode::NOT_FOUND, "experiment not found");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
