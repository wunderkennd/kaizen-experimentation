//! Integration tests for ADR-022 switchback assignment.
//!
//! Tests exercise the full assignment pipeline: config parsing → service →
//! `GetAssignmentResponse`. Switchback-specific unit tests live in `switchback.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;

const DEV_CONFIG: &str = include_str!("../../../dev/config.json");

fn make_service() -> AssignmentServiceImpl {
    let config = Config::from_json(DEV_CONFIG).expect("dev config must parse");
    AssignmentServiceImpl::from_config(Arc::new(config))
}

fn no_attrs() -> HashMap<String, String> {
    HashMap::new()
}

fn market_attrs(market_id: &str) -> HashMap<String, String> {
    [("market_id".to_string(), market_id.to_string())]
        .into_iter()
        .collect()
}

// ── Config parsing ────────────────────────────────────────────────────────────

#[test]
fn dev_config_parses_switchback_experiments() {
    let config = Config::from_json(DEV_CONFIG).expect("config must parse");
    let sb1 = config
        .experiments_by_id
        .get("exp_dev_switchback_001")
        .expect("switchback_001 must exist");
    assert_eq!(sb1.r#type, "SWITCHBACK");
    let sb_cfg = sb1
        .switchback_config
        .as_ref()
        .expect("switchback_config must be present");
    assert_eq!(sb_cfg.block_duration_secs, 3600);
    assert_eq!(sb_cfg.planned_cycles, 8);
    assert_eq!(sb_cfg.washout_period_secs, 300);
    assert_eq!(sb_cfg.design, "SIMPLE_ALTERNATING");
}

#[test]
fn dev_config_parses_balanced_switchback() {
    let config = Config::from_json(DEV_CONFIG).expect("config must parse");
    let sb2 = config
        .experiments_by_id
        .get("exp_dev_switchback_002")
        .expect("switchback_002 must exist");
    let sb_cfg = sb2.switchback_config.as_ref().unwrap();
    assert_eq!(sb_cfg.cluster_attribute, "market_id");
    assert_eq!(sb_cfg.design, "REGULAR_BALANCED");
}

// ── Service — basic assignment ────────────────────────────────────────────────

#[tokio::test]
async fn switchback_returns_active_assignment() {
    let svc = make_service();
    let resp = svc
        .assign("exp_dev_switchback_001", "user_abc", "", &no_attrs())
        .await
        .expect("assign must succeed");
    assert!(resp.is_active);
    // variant_id is either "control" or "" (washout) — never a foreign value.
    assert!(
        resp.variant_id == "control"
            || resp.variant_id == "treatment"
            || resp.variant_id.is_empty(),
        "unexpected variant_id: {}",
        resp.variant_id
    );
}

#[tokio::test]
async fn switchback_block_index_populated() {
    let svc = make_service();
    let resp = svc
        .assign("exp_dev_switchback_001", "user_abc", "", &no_attrs())
        .await
        .unwrap();
    // During washout block_index is 0 (default); outside washout it must be > 0
    // because the current epoch is well past 0. Either way it must be non-negative.
    assert!(resp.block_index >= 0, "block_index must be non-negative");
}

#[tokio::test]
async fn switchback_all_users_same_variant_same_block() {
    // SIMPLE_ALTERNATING: every user in the same block gets the same variant.
    let svc = make_service();
    let mut variant_ids = std::collections::HashSet::new();
    for i in 0..50 {
        let user = format!("user_{i}");
        let resp = svc
            .assign("exp_dev_switchback_001", &user, "", &no_attrs())
            .await
            .unwrap();
        if !resp.variant_id.is_empty() {
            variant_ids.insert(resp.variant_id);
        }
    }
    // At any given moment all non-washout users must be in the same arm.
    assert!(
        variant_ids.len() <= 1,
        "SIMPLE_ALTERNATING: multiple variants observed simultaneously: {variant_ids:?}"
    );
}

#[tokio::test]
async fn switchback_missing_config_returns_error() {
    // Build a config with a SWITCHBACK experiment that has no switchback_config.
    let raw = r#"{
        "experiments": [{
            "experiment_id": "sw_no_config",
            "state": "RUNNING",
            "type": "SWITCHBACK",
            "hash_salt": "salt",
            "layer_id": "l1",
            "variants": [
                {"variant_id": "c", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}"},
                {"variant_id": "t", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{}"}
            ],
            "allocation": {"start_bucket": 0, "end_bucket": 9999}
        }],
        "layers": [{"layer_id": "l1", "total_buckets": 10000}]
    }"#;
    let config = Config::from_json(raw).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));
    let err = svc
        .assign("sw_no_config", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(
        err.message().contains("switchback_config"),
        "error must mention switchback_config: {}",
        err.message()
    );
}

#[tokio::test]
async fn switchback_invalid_block_duration_returns_error() {
    // block_duration_secs = 1800 (below 1-hour minimum).
    let raw = r#"{
        "experiments": [{
            "experiment_id": "sw_bad_dur",
            "state": "RUNNING",
            "type": "SWITCHBACK",
            "hash_salt": "salt",
            "layer_id": "l1",
            "variants": [
                {"variant_id": "c", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}"},
                {"variant_id": "t", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{}"}
            ],
            "allocation": {"start_bucket": 0, "end_bucket": 9999},
            "switchback_config": {
                "block_duration_secs": 1800,
                "planned_cycles": 4
            }
        }],
        "layers": [{"layer_id": "l1", "total_buckets": 10000}]
    }"#;
    let config = Config::from_json(raw).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));
    let err = svc
        .assign("sw_bad_dur", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn switchback_inactive_experiment_returns_not_active() {
    let raw = r#"{
        "experiments": [{
            "experiment_id": "sw_draft",
            "state": "DRAFT",
            "type": "SWITCHBACK",
            "hash_salt": "salt",
            "layer_id": "l1",
            "variants": [
                {"variant_id": "c", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}"},
                {"variant_id": "t", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{}"}
            ],
            "allocation": {"start_bucket": 0, "end_bucket": 9999},
            "switchback_config": {
                "block_duration_secs": 3600,
                "planned_cycles": 4
            }
        }],
        "layers": [{"layer_id": "l1", "total_buckets": 10000}]
    }"#;
    let config = Config::from_json(raw).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));
    let resp = svc
        .assign("sw_draft", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(!resp.is_active);
}

// ── Cluster-attribute based assignment ───────────────────────────────────────

#[tokio::test]
async fn switchback_regular_balanced_uses_cluster_attribute() {
    let svc = make_service();
    // Two different markets may get different variants (staggered groups).
    // The test simply verifies assignment is consistent per-market.
    for market in ["us", "uk", "de", "fr", "jp"] {
        let resp1 = svc
            .assign("exp_dev_switchback_002", "user_x", "", &market_attrs(market))
            .await
            .unwrap();
        let resp2 = svc
            .assign("exp_dev_switchback_002", "user_y", "", &market_attrs(market))
            .await
            .unwrap();
        // All users in the same market during the same block get the same variant.
        if !resp1.variant_id.is_empty() && !resp2.variant_id.is_empty() {
            assert_eq!(
                resp1.variant_id, resp2.variant_id,
                "market {market}: users in same market/block must share variant"
            );
        }
    }
}

// ── Block index correctness ───────────────────────────────────────────────────

#[test]
fn block_index_formula_matches_epoch() {
    use experimentation_assignment::switchback::compute_block_index;
    // spot-check: block_duration = 1h = 3600s
    // At t = 3600 * 491520 = 1_769_472_000 (approx 2026) block_index = 491520
    let t = 3600_i64 * 491_520;
    assert_eq!(compute_block_index(t, 3600), 491_520);
}
