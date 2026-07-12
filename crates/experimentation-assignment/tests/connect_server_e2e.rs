//! ADR-031 pilot — in-process round-trip tests for the unary RPCs on
//! `AssignmentService` over Connect/JSON.
//!
//! Binds the ConnectRPC server on 127.0.0.1:0, POSTs Connect/JSON to
//! `/experimentation.assignment.v1.AssignmentService/{method}`, and asserts
//! the JSON shape matches the tonic/http_json contract for each method that
//! has one. `GetInterleavedList` had no hand-rolled JSON path before this
//! PR — the round-trip test here closes that coverage gap (#642).

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
async fn connect_get_assignments_returns_batch() {
    let (addr, _svc) = start_connect_server().await;

    let body = serde_json::json!({
        "userId": "test-user-1",
        "sessionId": "sess-1",
        "attributes": {},
    });
    let (status, resp) = connect_json_post(addr, "GetAssignments", body).await;

    assert_eq!(status, 200, "GetAssignments failed: {resp}");
    // Two-phase batch always returns a (possibly empty) assignments array.
    // dev/config.json has 13 running experiments so we expect >0 here — a
    // dropped/empty batch would silently regress the ADR-014 evaluator.
    let assignments = resp
        .get("assignments")
        .and_then(|v| v.as_array())
        .expect("assignments array missing");
    assert!(
        !assignments.is_empty(),
        "expected at least one assignment for dev/config.json, got empty batch",
    );
}

#[tokio::test]
async fn connect_get_interleaved_list_merges_two_algorithms() {
    let (addr, _svc) = start_connect_server().await;

    // exp_dev_004 is the TEAM_DRAFT interleaving experiment in dev/config.json
    // (algorithm_ids: ["algo_a", "algo_b"]). Two disjoint ranked lists let us
    // assert the merged output is populated without knowing the exact draft
    // order (which is seed-derived and would over-fit the test).
    let body = serde_json::json!({
        "experimentId": "exp_dev_004",
        "userId": "test-user-interleave",
        "algorithmLists": {
            "algo_a": {"itemIds": ["a1", "a2", "a3"]},
            "algo_b": {"itemIds": ["b1", "b2", "b3"]},
        },
    });
    let (status, resp) = connect_json_post(addr, "GetInterleavedList", body).await;

    assert_eq!(status, 200, "GetInterleavedList failed: {resp}");
    let merged = resp
        .get("mergedList")
        .and_then(|v| v.as_array())
        .expect("mergedList array missing");
    assert!(!merged.is_empty(), "expected merged list, got empty");
    // Provenance carries the algorithm-id contribution map for each item
    // (M4a needs this for the interleaving contribution analysis).
    assert!(
        resp.get("provenance").is_some(),
        "missing provenance: {resp}",
    );
}

#[tokio::test]
async fn connect_get_slate_assignment_returns_ordered_slate() {
    let (addr, _svc) = start_connect_server().await;

    // exp_dev_slate_001 is the SLATE_FACTORIZED_TS experiment with num_slots=3
    // and candidate_pool_size=6. Six candidate item IDs saturate the pool
    // and let the assign_slate fallback produce a valid slate even without
    // a live M4b bandit backend.
    let body = serde_json::json!({
        "userId": "test-user-slate",
        "experimentId": "exp_dev_slate_001",
        "candidateItemIds": ["i1","i2","i3","i4","i5","i6"],
        "attributes": {},
    });
    let (status, resp) = connect_json_post(addr, "GetSlateAssignment", body).await;

    assert_eq!(status, 200, "GetSlateAssignment failed: {resp}");
    let slate = resp
        .get("slateItemIds")
        .and_then(|v| v.as_array())
        .expect("slateItemIds missing");
    assert_eq!(slate.len(), 3, "num_slots=3 → slate length must be 3");
    let probs = resp
        .get("slotProbabilities")
        .and_then(|v| v.as_array())
        .expect("slotProbabilities missing");
    assert_eq!(probs.len(), 3, "one probability entry per slot");
}

#[tokio::test]
async fn connect_stream_config_updates_still_unimplemented() {
    let (addr, _svc) = start_connect_server().await;

    // StreamConfigUpdates is server-streaming and lands in #643. Until then
    // it must return Unimplemented (HTTP 501). This test is the negative
    // guard that flips green once #643 wires the stream.
    let body = serde_json::json!({"lastKnownVersion": 0});
    let (status, _resp) =
        connect_json_post(addr, "StreamConfigUpdates", body).await;
    assert_eq!(
        status, 501,
        "StreamConfigUpdates should still be Unimplemented until #643",
    );
}
