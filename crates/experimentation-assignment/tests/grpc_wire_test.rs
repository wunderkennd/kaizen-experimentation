//! M1 over native gRPC on the production listener (ADR-032 step 0).
//!
//! A tonic client — the same stack M5, M4b and Go connect-go clients
//! interoperate with — talks h2c to [`connect_server::app`], so these tests
//! prove the Connect listener is a drop-in for the retired tonic server:
//! unary calls, error codes, `grpc.health.v1`, and server-streaming
//! `StreamConfigUpdates` (ordering, fan-out, clean disconnect). Connect JSON
//! coverage lives in `connect_server_e2e.rs`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::connect_server;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_proto::experimentation::assignment::v1::{
    ConfigUpdate, GetAssignmentRequest, StreamConfigUpdatesRequest,
    assignment_service_client::AssignmentServiceClient,
};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::{health_check_response::ServingStatus, health_client::HealthClient};

async fn serve(svc: Arc<AssignmentServiceImpl>) -> SocketAddr {
    let (app, _health) = connect_server::app(svc);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

async fn channel(addr: SocketAddr) -> Channel {
    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("h2c connect")
}

fn empty_service() -> Arc<AssignmentServiceImpl> {
    // Empty config is fine for the streaming tests — StreamConfigUpdates
    // doesn't touch the assignment path.
    let cfg = Config {
        experiments: Vec::new(),
        layers: Vec::new(),
        experiments_by_id: Default::default(),
        layers_by_id: Default::default(),
    };
    Arc::new(AssignmentServiceImpl::from_config(Arc::new(cfg)))
}

fn dev_service() -> Arc<AssignmentServiceImpl> {
    let path = ["dev/config.json", "../../dev/config.json"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.exists())
        .expect("cannot find dev/config.json");
    let config = Config::from_file(path).expect("dev/config.json should parse");
    Arc::new(AssignmentServiceImpl::from_config(Arc::new(config)))
}

fn update(version: i64, is_deletion: bool) -> ConfigUpdate {
    ConfigUpdate {
        experiment: None,
        is_deletion,
        version,
    }
}

#[tokio::test]
async fn grpc_get_assignment_matches_domain_result() {
    let svc = dev_service();
    let expected = svc
        .assign("exp_dev_001", "user_grpc_wire", "", &HashMap::new())
        .await
        .expect("domain assign");
    let addr = serve(svc).await;

    let mut client = AssignmentServiceClient::new(channel(addr).await);
    let got = client
        .get_assignment(GetAssignmentRequest {
            experiment_id: "exp_dev_001".into(),
            user_id: "user_grpc_wire".into(),
            ..Default::default()
        })
        .await
        .expect("gRPC GetAssignment")
        .into_inner();

    assert_eq!(got, expected);
}

#[tokio::test]
async fn grpc_unknown_experiment_is_not_found() {
    let addr = serve(dev_service()).await;
    let mut client = AssignmentServiceClient::new(channel(addr).await);
    let status = client
        .get_assignment(GetAssignmentRequest {
            experiment_id: "no_such_experiment".into(),
            user_id: "u".into(),
            ..Default::default()
        })
        .await
        .expect_err("unknown experiment must fail");
    assert_eq!(status.code(), tonic::Code::NotFound);
}

/// `grpc_health_probe -addr=:50051` (the ECS HEALTHCHECK) checks the empty
/// service name; per-service checks name AssignmentService.
#[tokio::test]
async fn grpc_health_reports_serving() {
    let addr = serve(empty_service()).await;
    let mut health = HealthClient::new(channel(addr).await);
    for service in ["", "experimentation.assignment.v1.AssignmentService"] {
        let resp = health
            .check(HealthCheckRequest {
                service: service.into(),
            })
            .await
            .unwrap_or_else(|e| panic!("health check {service:?}: {e}"))
            .into_inner();
        assert_eq!(resp.status(), ServingStatus::Serving, "service {service:?}");
    }
}

/// Ordering — a single subscriber sees updates in the order they were pushed.
#[tokio::test]
async fn stream_delivers_updates_in_push_order() {
    let svc = empty_service();
    let mut client = AssignmentServiceClient::new(channel(serve(svc.clone()).await).await);

    let mut stream = client
        .stream_config_updates(StreamConfigUpdatesRequest {
            last_known_version: 0,
        })
        .await
        .expect("stream open")
        .into_inner();

    // The handler subscribes before response headers go out, so pushes after
    // the call returns are observed. Pre-subscribe pushes go to zero
    // receivers: the M5 client reconnects with last_known_version rather than
    // expecting server-side replay.
    svc.push_config_update(update(1, false));
    svc.push_config_update(update(2, true));
    svc.push_config_update(update(3, false));

    for (expected_version, expected_deletion) in [(1, false), (2, true), (3, false)] {
        let got = stream
            .next()
            .await
            .expect("stream not exhausted")
            .expect("no lag");
        assert_eq!(got.version, expected_version);
        assert_eq!(got.is_deletion, expected_deletion);
    }
}

/// Fan-out — every subscriber receives every event pushed after it subscribed.
#[tokio::test]
async fn broadcast_fan_out_delivers_to_all_subscribers() {
    let svc = empty_service();
    let ch = channel(serve(svc.clone()).await).await;
    let mut a = AssignmentServiceClient::new(ch.clone())
        .stream_config_updates(StreamConfigUpdatesRequest::default())
        .await
        .unwrap()
        .into_inner();
    let mut b = AssignmentServiceClient::new(ch)
        .stream_config_updates(StreamConfigUpdatesRequest::default())
        .await
        .unwrap()
        .into_inner();

    svc.push_config_update(update(7, false));

    assert_eq!(a.next().await.unwrap().unwrap().version, 7);
    assert_eq!(b.next().await.unwrap().unwrap().version, 7);
}

/// Clean disconnect — dropping one client stream leaves other subscribers and
/// the sender working.
#[tokio::test]
async fn dropping_one_subscriber_leaves_others_working() {
    let svc = empty_service();
    let ch = channel(serve(svc.clone()).await).await;
    let mut keep = AssignmentServiceClient::new(ch.clone())
        .stream_config_updates(StreamConfigUpdatesRequest::default())
        .await
        .unwrap()
        .into_inner();
    {
        let _drop_me = AssignmentServiceClient::new(ch)
            .stream_config_updates(StreamConfigUpdatesRequest::default())
            .await
            .unwrap()
            .into_inner();
    }

    svc.push_config_update(update(42, false));
    assert_eq!(keep.next().await.unwrap().unwrap().version, 42);
}

/// `push_config_update` returns 0 when nobody's subscribed — a valid "M5
/// pushed while no client was listening" event, not an error.
#[tokio::test]
async fn push_without_subscribers_is_silent_ok() {
    let svc = empty_service();
    assert_eq!(svc.push_config_update(update(1, false)), 0);
}
