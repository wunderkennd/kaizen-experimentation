//! ADR-031 pilot — in-process round-trip test for GetAssignment over Connect.
//!
//! Binds the ConnectRPC server on 127.0.0.1:0, POSTs a Connect/JSON request to
//! `/experimentation.assignment.v1.AssignmentService/GetAssignment`, and
//! asserts the response matches the existing tonic/http_json contract.

#![cfg(feature = "connectrpc")]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::connect_server::ConnectAssignment;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_proto_connect::experimentation::assignment::v1::AssignmentServiceExt;

async fn start_connect_server() -> (SocketAddr, Arc<AssignmentServiceImpl>) {
    let path = if Path::new("dev/config.json").exists() {
        Path::new("dev/config.json")
    } else if Path::new("../../dev/config.json").exists() {
        Path::new("../../dev/config.json")
    } else {
        panic!("cannot find dev/config.json from {:?}", std::env::current_dir().unwrap());
    };
    let config = Config::from_file(path).expect("dev/config.json should parse");
    let svc = Arc::new(AssignmentServiceImpl::from_config(Arc::new(config)));
    let connect_svc = Arc::new(ConnectAssignment::new(svc.clone()));
    let router = connect_svc.register(connectrpc::Router::new());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let bound = connectrpc::Server::from_listener(listener);

    tokio::spawn(async move {
        let _ = bound.serve(router).await;
    });

    (addr, svc)
}

async fn connect_json_post(addr: SocketAddr, method: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use hyper::Request;
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpStream;

    let path = format!("/experimentation.assignment.v1.AssignmentService/{method}");
    let body_bytes = serde_json::to_vec(&body).unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });

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
    let json: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn connect_get_assignment_round_trip_matches_http_json_contract() {
    let (addr, _svc) = start_connect_server().await;

    // dev/config.json contains an experiment used by the existing http_json e2e
    // suite — reuse its inputs so the assertion is the same contract.
    let body = serde_json::json!({
        "userId": "test-user-1",
        "experimentId": "exp_dev_001",
        "sessionId": "sess-1",
        "attributes": {},
    });

    let (status, resp) = connect_json_post(addr, "GetAssignment", body).await;

    assert_eq!(status, 200, "Connect call failed: {resp:?}");
    assert!(resp.get("experimentId").is_some(), "missing experimentId: {resp}");
    // is_active is the load-bearing assertion shared with http_json's contract.
    assert!(
        resp.get("isActive").and_then(|v| v.as_bool()).is_some(),
        "missing isActive: {resp}",
    );
}

#[tokio::test]
async fn connect_get_assignment_unknown_experiment_returns_not_found() {
    let (addr, _svc) = start_connect_server().await;

    let body = serde_json::json!({
        "userId": "u1",
        "experimentId": "definitely-not-an-experiment",
        "sessionId": "s1",
        "attributes": {},
    });

    let (status, _resp) = connect_json_post(addr, "GetAssignment", body).await;

    // Connect maps NotFound to HTTP 404 in its protocol; the body carries
    // {"code":"not_found","message":"..."} but we only assert the status here.
    assert_eq!(status, 404);
}

#[tokio::test]
async fn connect_other_rpcs_return_unimplemented() {
    let (addr, _svc) = start_connect_server().await;

    let body = serde_json::json!({"userId":"u1","sessionId":"s1","attributes":{}});
    let (status, _resp) = connect_json_post(addr, "GetAssignments", body).await;

    // Connect maps Unimplemented to HTTP 501.
    assert_eq!(status, 501, "other RPCs should be Unimplemented until #642/#643");
}
