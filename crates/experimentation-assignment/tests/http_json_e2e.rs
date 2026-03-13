//! End-to-end integration tests for the JSON HTTP API.
//!
//! These tests start the HTTP server in-process with a real config, then make
//! HTTP requests and verify the full roundtrip: JSON request → HTTP handler →
//! assign() → JSON response.
//!
//! Tests validate that SDK clients will receive correct assignment data when
//! talking to the real Assignment Service over HTTP.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use http_body_util::{BodyExt, Full};
use tokio::net::TcpStream;

use experimentation_assignment::config::Config;
use experimentation_assignment::http_json;
use experimentation_assignment::service::AssignmentServiceImpl;

/// Start the HTTP JSON server on a random port and return the address.
async fn start_server(svc: Arc<AssignmentServiceImpl>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let svc = svc.clone();
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let svc = svc.clone();
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        io,
                        hyper::service::service_fn(move |req| {
                            let svc = svc.clone();
                            http_json::__test_handle_request(svc, req)
                        }),
                    )
                    .await;
            });
        }
    });

    addr
}

/// Make a JSON POST request and return the response body as a parsed JSON value.
async fn json_post(addr: SocketAddr, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let stream = TcpStream::connect(addr).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();

    tokio::spawn(async move {
        let _ = conn.await;
    });

    let body_bytes = serde_json::to_vec(body).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("host", format!("127.0.0.1:{}", addr.port()))
        .body(Full::new(Bytes::from(body_bytes)))
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn load_test_config() -> Arc<AssignmentServiceImpl> {
    // Integration tests run from the crate root or workspace root — try both paths.
    let path = if Path::new("dev/config.json").exists() {
        Path::new("dev/config.json")
    } else if Path::new("../../dev/config.json").exists() {
        Path::new("../../dev/config.json")
    } else {
        panic!("cannot find dev/config.json from {:?}", std::env::current_dir().unwrap());
    };
    let config = Config::from_file(path).expect("dev/config.json should parse");
    Arc::new(AssignmentServiceImpl::from_config(Arc::new(config)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_get_assignment_returns_active_assignment() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-e2e-001",
            "experimentId": "exp_dev_001"
        }),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(json["experimentId"], "exp_dev_001");
    assert_eq!(json["isActive"], true);
    // Must have a non-empty variant
    let variant = json["variantId"].as_str().unwrap();
    assert!(variant == "control" || variant == "treatment", "unexpected variant: {variant}");
}

#[tokio::test]
async fn e2e_get_assignment_deterministic() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let body = serde_json::json!({
        "userId": "user-deterministic-42",
        "experimentId": "exp_dev_001"
    });
    let path = "/experimentation.assignment.v1.AssignmentService/GetAssignment";

    let (_, json1) = json_post(addr, path, &body).await;
    let (_, json2) = json_post(addr, path, &body).await;

    // Same user + experiment → same variant
    assert_eq!(json1["variantId"], json2["variantId"]);
}

#[tokio::test]
async fn e2e_get_assignment_not_found() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-1",
            "experimentId": "nonexistent_experiment"
        }),
    )
    .await;

    assert_eq!(status, 404);
    assert!(json["message"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn e2e_get_assignment_with_targeting() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    // exp_dev_002 requires country IN [US, UK] AND tier IN [premium, platinum]
    // User with both matching attributes should get assigned
    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-us-premium",
            "experimentId": "exp_dev_002",
            "attributes": { "country": "US", "tier": "premium" }
        }),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(json["isActive"], true);
    let variant = json["variantId"].as_str().unwrap();
    assert!(!variant.is_empty(), "targeted user should get a variant");
}

#[tokio::test]
async fn e2e_get_assignment_targeting_miss() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    // User without matching attributes → empty variant (targeting miss)
    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-no-targeting",
            "experimentId": "exp_dev_002",
            "attributes": { "country": "JP", "tier": "free" }
        }),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(json["isActive"], true);
    // Targeting miss → empty variant_id
    assert_eq!(json["variantId"].as_str().unwrap_or(""), "");
}

#[tokio::test]
async fn e2e_get_assignments_bulk() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignments",
        &serde_json::json!({
            "userId": "user-bulk-001",
            "attributes": { "country": "US", "tier": "premium" }
        }),
    )
    .await;

    assert_eq!(status, 200);
    let assignments = json["assignments"].as_array().unwrap();
    // Should have multiple experiments
    assert!(assignments.len() >= 2, "expected >= 2 assignments, got {}", assignments.len());

    // All should have experimentId
    for a in assignments {
        assert!(a["experimentId"].is_string());
    }
}

#[tokio::test]
async fn e2e_get_assignments_bulk_has_active_experiments() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (_, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignments",
        &serde_json::json!({
            "userId": "user-bulk-002"
        }),
    )
    .await;

    let assignments = json["assignments"].as_array().unwrap();
    // exp_dev_001 (no targeting) should always be present and active
    let exp001 = assignments.iter().find(|a| a["experimentId"] == "exp_dev_001");
    assert!(exp001.is_some(), "exp_dev_001 should be in bulk response");
    assert_eq!(exp001.unwrap()["isActive"], true);
}

#[tokio::test]
async fn e2e_payload_json_roundtrip() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-payload-test",
            "experimentId": "exp_dev_001"
        }),
    )
    .await;

    assert_eq!(status, 200);
    // payloadJson should be a valid JSON string
    let payload_str = json["payloadJson"].as_str().unwrap();
    // Either "{}" for control or {"feature": true} for treatment
    let _payload: serde_json::Value = serde_json::from_str(payload_str)
        .expect("payloadJson should be valid JSON");
}

#[tokio::test]
async fn e2e_method_not_allowed() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    // Make a GET request (only POST is supported)
    let stream = TcpStream::connect(addr).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move { let _ = conn.await; });

    let req = Request::builder()
        .method("GET")
        .uri("/experimentation.assignment.v1.AssignmentService/GetAssignment")
        .header("host", format!("127.0.0.1:{}", addr.port()))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    assert_eq!(resp.status().as_u16(), 405);
}

#[tokio::test]
async fn e2e_unknown_path_returns_404() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (status, json) = json_post(
        addr,
        "/unknown/path",
        &serde_json::json!({}),
    )
    .await;

    assert_eq!(status, 404);
    assert!(json["message"].as_str().unwrap().contains("unknown"));
}

#[tokio::test]
async fn e2e_invalid_json_returns_400() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    // Send malformed body
    let stream = TcpStream::connect(addr).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move { let _ = conn.await; });

    let req = Request::builder()
        .method("POST")
        .uri("/experimentation.assignment.v1.AssignmentService/GetAssignment")
        .header("content-type", "application/json")
        .header("host", format!("127.0.0.1:{}", addr.port()))
        .body(Full::new(Bytes::from("not json")))
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn e2e_cors_preflight() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move { let _ = conn.await; });

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/experimentation.assignment.v1.AssignmentService/GetAssignment")
        .header("host", format!("127.0.0.1:{}", addr.port()))
        .header("origin", "https://example.com")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    assert_eq!(resp.status().as_u16(), 204);
    assert_eq!(resp.headers()["access-control-allow-origin"], "*");
    assert!(resp.headers()["access-control-allow-methods"]
        .to_str()
        .unwrap()
        .contains("POST"));
}

#[tokio::test]
async fn e2e_session_experiment_requires_session_id() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    // exp_dev_003 is SESSION_LEVEL — requires session_id
    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-session-001",
            "experimentId": "exp_dev_003"
        }),
    )
    .await;

    // Should fail with 400 because session_id is empty
    assert_eq!(status, 400);
    assert!(json["message"].as_str().unwrap().contains("session_id"));
}

#[tokio::test]
async fn e2e_session_experiment_with_session_id() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let (status, json) = json_post(
        addr,
        "/experimentation.assignment.v1.AssignmentService/GetAssignment",
        &serde_json::json!({
            "userId": "user-session-002",
            "experimentId": "exp_dev_003",
            "sessionId": "session-abc-123"
        }),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(json["isActive"], true);
    let variant = json["variantId"].as_str().unwrap();
    assert!(variant == "control" || variant == "treatment");
}

#[tokio::test]
async fn e2e_user_distribution_sanity() {
    let svc = load_test_config();
    let addr = start_server(svc).await;

    let mut control_count = 0;
    let mut treatment_count = 0;

    for i in 0..200 {
        let (_, json) = json_post(
            addr,
            "/experimentation.assignment.v1.AssignmentService/GetAssignment",
            &serde_json::json!({
                "userId": format!("dist-user-{}", i),
                "experimentId": "exp_dev_001"
            }),
        )
        .await;

        match json["variantId"].as_str().unwrap() {
            "control" => control_count += 1,
            "treatment" => treatment_count += 1,
            other => panic!("unexpected variant: {other}"),
        }
    }

    // With 50/50 split, expect roughly even. Allow wide margin for small sample.
    assert!(control_count > 50, "control too low: {control_count}");
    assert!(treatment_count > 50, "treatment too low: {treatment_count}");
}
