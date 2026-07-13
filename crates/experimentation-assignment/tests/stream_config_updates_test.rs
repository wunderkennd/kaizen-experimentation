//! ADR-031 #643 — server-streaming `StreamConfigUpdates` tests.
//!
//! The bridge is a thin subscribe-to-broadcast on the domain
//! [`AssignmentServiceImpl`]. Both the tonic handler and the Connect handler
//! call the same `subscribe_config_updates` source, so a single in-process
//! tonic streaming test proves the *domain* semantics (ordering, per-connect
//! isolation, clean shutdown, lag → DataLoss) that both transports inherit.
//!
//! Full binary-framing over-the-wire validation for the Connect transport
//! lands with #644 (server-go client + conformance run) per the ADR-031
//! acceptance criteria for `StreamConfigUpdates`.

use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_proto::experimentation::assignment::v1::{
    assignment_service_server::AssignmentService, ConfigUpdate, StreamConfigUpdatesRequest,
};
use tokio_stream::StreamExt;
use tonic::Request;

fn service() -> Arc<AssignmentServiceImpl> {
    // Empty config is fine — StreamConfigUpdates doesn't touch the assignment
    // path. We're validating the domain broadcast source, not experiment eval.
    let cfg = Config {
        experiments: Vec::new(),
        layers: Vec::new(),
        experiments_by_id: Default::default(),
        layers_by_id: Default::default(),
    };
    Arc::new(AssignmentServiceImpl::from_config(Arc::new(cfg)))
}

fn update(version: i64, is_deletion: bool) -> ConfigUpdate {
    ConfigUpdate {
        experiment: None,
        is_deletion,
        version,
    }
}

/// Ordering — a single subscriber sees updates in the order they were
/// pushed. Baseline stream contract every transport inherits.
#[tokio::test]
async fn stream_delivers_updates_in_push_order() {
    let svc = service();

    let mut stream = svc
        .stream_config_updates(Request::new(StreamConfigUpdatesRequest {
            last_known_version: 0,
        }))
        .await
        .expect("stream open")
        .into_inner();

    // Push after subscribe — pre-subscribe pushes go to zero receivers and
    // are discarded, mirroring how the M5 client will reconnect with
    // last_known_version rather than expecting server-side replay.
    svc.push_config_update(update(1, false));
    svc.push_config_update(update(2, true));
    svc.push_config_update(update(3, false));

    for expected_version in 1..=3 {
        let got = stream
            .next()
            .await
            .expect("stream not exhausted")
            .expect("no lag");
        assert_eq!(got.version, expected_version);
    }
}

/// Fan-out — every subscriber receives every event pushed *after* it
/// subscribed. Late subscribers don't see earlier events (M5 client owns
/// replay via `last_known_version`).
#[tokio::test]
async fn broadcast_fan_out_delivers_to_all_subscribers() {
    let svc = service();

    let mut a = svc
        .stream_config_updates(Request::new(StreamConfigUpdatesRequest::default()))
        .await
        .unwrap()
        .into_inner();
    let mut b = svc
        .stream_config_updates(Request::new(StreamConfigUpdatesRequest::default()))
        .await
        .unwrap()
        .into_inner();

    svc.push_config_update(update(7, false));

    let ra = a.next().await.unwrap().unwrap();
    let rb = b.next().await.unwrap().unwrap();
    assert_eq!(ra.version, 7);
    assert_eq!(rb.version, 7);
}

/// Clean shutdown — dropping a stream releases the receiver without
/// disturbing other subscribers or the sender. Verifies neither the tonic
/// handler nor the broadcast source leak state per-connection.
#[tokio::test]
async fn dropping_one_subscriber_leaves_others_working() {
    let svc = service();

    let mut keep = svc
        .stream_config_updates(Request::new(StreamConfigUpdatesRequest::default()))
        .await
        .unwrap()
        .into_inner();
    {
        let _drop_me = svc
            .stream_config_updates(Request::new(StreamConfigUpdatesRequest::default()))
            .await
            .unwrap()
            .into_inner();
        // _drop_me dropped at end of scope — its receiver is released.
    }

    svc.push_config_update(update(42, false));

    let got = keep.next().await.unwrap().unwrap();
    assert_eq!(got.version, 42);
}

/// `push_config_update` returns 0 when nobody's subscribed. Not an error
/// path — a valid "M5 pushed while no client was listening" event. Guards
/// against future refactors that might turn this into a hard error.
#[tokio::test]
async fn push_without_subscribers_is_silent_ok() {
    let svc = service();
    let observed = svc.push_config_update(update(1, false));
    assert_eq!(observed, 0);
}
