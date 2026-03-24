use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_proto::experimentation::assignment::v1::{
    assignment_service_server::AssignmentService, GetAssignmentsRequest, RankedList,
};
use experimentation_proto::experimentation::bandit::v1::bandit_policy_service_server::{
    BanditPolicyService, BanditPolicyServiceServer,
};
use experimentation_proto::experimentation::bandit::v1::{
    CreateColdStartBanditRequest, CreateColdStartBanditResponse, ExportAffinityScoresRequest,
    ExportAffinityScoresResponse, GetPolicySnapshotRequest, RollbackPolicyRequest,
    SelectArmRequest, SlateAssignmentRequest, SlateAssignmentResponse,
};
use experimentation_proto::experimentation::common::v1::{ArmSelection, PolicySnapshot};

/// Compile-time embed of dev config — avoids path issues in tests.
const DEV_CONFIG: &str = include_str!("../../../dev/config.json");

fn make_service() -> AssignmentServiceImpl {
    let config = Config::from_json(DEV_CONFIG).expect("dev config must parse");
    AssignmentServiceImpl::from_config(Arc::new(config))
}

fn no_attrs() -> HashMap<String, String> {
    HashMap::new()
}

fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ── M1.2 Tests (static bucketing) ──

#[tokio::test]
async fn determinism_same_user_same_variant() {
    let svc = make_service();
    let r1 = svc
        .assign("exp_dev_001", "user_42", "", &no_attrs())
        .await
        .unwrap();
    let r2 = svc
        .assign("exp_dev_001", "user_42", "", &no_attrs())
        .await
        .unwrap();
    assert_eq!(
        r1.variant_id, r2.variant_id,
        "same user must get same variant"
    );
    assert_eq!(r1.payload_json, r2.payload_json);
    assert!(r1.is_active);
}

#[tokio::test]
async fn balance_50_50_chi_squared() {
    let svc = make_service();
    let mut counts = std::collections::HashMap::new();

    for i in 0..10_000 {
        let user_id = format!("balance_user_{i}");
        let resp = svc
            .assign("exp_dev_001", &user_id, "", &no_attrs())
            .await
            .unwrap();
        *counts.entry(resp.variant_id).or_insert(0u64) += 1;
    }

    // With 50/50 split over 10K users, expect ~5000 each.
    // Chi-squared test with df=1, threshold 6.635 (p > 0.01).
    let expected = 5000.0_f64;
    let chi_sq: f64 = counts
        .values()
        .map(|&observed| {
            let diff = observed as f64 - expected;
            (diff * diff) / expected
        })
        .sum();

    assert!(
        chi_sq < 6.635,
        "chi-squared {chi_sq} exceeds 6.635 (p<0.01) — balance is off. counts: {counts:?}"
    );
}

#[tokio::test]
async fn not_found_unknown_experiment() {
    let svc = make_service();
    let err = svc
        .assign("nonexistent_exp", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn empty_assignment_outside_allocation() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "narrow_exp",
            "state": "RUNNING",
            "hash_salt": "narrow_salt",
            "layer_id": "layer_narrow",
            "variants": [
                { "variant_id": "ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9 }
        }],
        "layers": [{ "layer_id": "layer_narrow", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let mut outside_count = 0;
    for i in 0..1000 {
        let resp = svc
            .assign("narrow_exp", &format!("user_{i}"), "", &no_attrs())
            .await
            .unwrap();
        if resp.variant_id.is_empty() {
            assert!(
                resp.is_active,
                "outside allocation should still be is_active=true"
            );
            outside_count += 1;
        }
    }

    assert!(
        outside_count > 900,
        "expected most users outside narrow allocation, got {outside_count}/1000 outside"
    );
}

#[tokio::test]
async fn inactive_experiment_returns_is_active_false() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "draft_exp",
            "state": "DRAFT",
            "hash_salt": "draft_salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let resp = svc
        .assign("draft_exp", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(
        !resp.is_active,
        "DRAFT experiment must return is_active=false"
    );
    assert!(
        resp.variant_id.is_empty(),
        "DRAFT experiment must not assign a variant"
    );
}

// ── M1.4 Tests (targeting rules) ──

#[tokio::test]
async fn targeting_country_in_match() {
    let svc = make_service();
    // exp_dev_002 targets country IN [US, UK] AND tier IN [premium, platinum]
    let resp = svc
        .assign(
            "exp_dev_002",
            "user_1",
            "",
            &attrs(&[("country", "US"), ("tier", "premium")]),
        )
        .await
        .unwrap();
    assert!(resp.is_active);
    assert!(
        !resp.variant_id.is_empty(),
        "targeted user should get a variant"
    );
}

#[tokio::test]
async fn targeting_empty_rule_matches_all() {
    let svc = make_service();
    // exp_dev_001 has no targeting rule — all users match
    let resp = svc
        .assign("exp_dev_001", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty());
}

#[tokio::test]
async fn targeting_missing_attribute_no_match() {
    let svc = make_service();
    // exp_dev_002 requires country + tier, but we provide neither
    let resp = svc
        .assign("exp_dev_002", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(
        resp.is_active,
        "targeting miss should still be is_active=true"
    );
    assert!(
        resp.variant_id.is_empty(),
        "targeting miss should return empty variant"
    );
}

#[tokio::test]
async fn targeting_no_rule_matches_all() {
    // Experiment without targeting_rule field at all
    let json = r#"{
        "experiments": [{
            "experiment_id": "no_target_exp",
            "state": "RUNNING",
            "hash_salt": "no_target_salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let resp = svc
        .assign("no_target_exp", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert!(
        !resp.variant_id.is_empty(),
        "no targeting rule → all users match"
    );
}

#[tokio::test]
async fn targeting_compound_and_across_groups() {
    let svc = make_service();
    // Correct country but wrong tier → AND fails
    let resp = svc
        .assign(
            "exp_dev_002",
            "user_1",
            "",
            &attrs(&[("country", "US"), ("tier", "free")]),
        )
        .await
        .unwrap();
    assert!(resp.variant_id.is_empty(), "wrong tier should not match");

    // Correct tier but wrong country → AND fails
    let resp2 = svc
        .assign(
            "exp_dev_002",
            "user_2",
            "",
            &attrs(&[("country", "FR"), ("tier", "premium")]),
        )
        .await
        .unwrap();
    assert!(
        resp2.variant_id.is_empty(),
        "wrong country should not match"
    );
}

#[tokio::test]
async fn targeting_gt_numeric() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "age_exp",
            "state": "RUNNING",
            "hash_salt": "age_salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "targeting_rule": {
                "groups": [{
                    "predicates": [{ "attribute_key": "age", "operator": "GT", "values": ["18"] }]
                }]
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let resp = svc
        .assign("age_exp", "user_1", "", &attrs(&[("age", "25")]))
        .await
        .unwrap();
    assert!(!resp.variant_id.is_empty(), "age 25 > 18 should match");

    let resp2 = svc
        .assign("age_exp", "user_2", "", &attrs(&[("age", "15")]))
        .await
        .unwrap();
    assert!(resp2.variant_id.is_empty(), "age 15 <= 18 should not match");
}

// ── M1.5 Tests (session-level + layer-aware assignment) ──

#[tokio::test]
async fn session_level_determinism() {
    let svc = make_service();
    let r1 = svc
        .assign("exp_dev_003", "user_1", "session_abc", &no_attrs())
        .await
        .unwrap();
    let r2 = svc
        .assign("exp_dev_003", "user_1", "session_abc", &no_attrs())
        .await
        .unwrap();
    assert_eq!(
        r1.variant_id, r2.variant_id,
        "same session_id must get same variant"
    );
    assert!(r1.is_active);
    assert!(!r1.variant_id.is_empty());
}

#[tokio::test]
async fn session_level_cross_session_variation() {
    let svc = make_service();
    // Collect variants across many sessions — should see both variants.
    let mut variants = std::collections::HashSet::new();
    for i in 0..200 {
        let session = format!("session_{i}");
        let resp = svc
            .assign("exp_dev_003", "user_1", &session, &no_attrs())
            .await
            .unwrap();
        variants.insert(resp.variant_id);
    }
    assert!(
        variants.len() >= 2,
        "expected at least 2 distinct variants across sessions, got {variants:?}"
    );
}

#[tokio::test]
async fn session_level_missing_session_id() {
    let svc = make_service();
    let err = svc
        .assign("exp_dev_003", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(
        err.message().contains("session_id"),
        "error should mention session_id: {}",
        err.message()
    );
}

#[tokio::test]
async fn session_level_same_user_different_sessions() {
    let svc = make_service();
    let r1 = svc
        .assign("exp_dev_003", "user_42", "sess_A", &no_attrs())
        .await
        .unwrap();
    let r2 = svc
        .assign("exp_dev_003", "user_42", "sess_B", &no_attrs())
        .await
        .unwrap();
    // Different sessions may get different buckets (hash depends on session_id, not user_id).
    // We can't guarantee different variants, but we verify both succeed.
    assert!(r1.is_active);
    assert!(r2.is_active);
    assert!(!r1.variant_id.is_empty());
    assert!(!r2.variant_id.is_empty());
}

#[tokio::test]
async fn layer_orthogonality() {
    // User gets assignment in both default layer (AB) and session layer (SESSION_LEVEL).
    let svc = make_service();
    let ab_resp = svc
        .assign("exp_dev_001", "user_1", "", &no_attrs())
        .await
        .unwrap();
    let session_resp = svc
        .assign("exp_dev_003", "user_1", "session_1", &no_attrs())
        .await
        .unwrap();
    assert!(ab_resp.is_active);
    assert!(session_resp.is_active);
    assert!(
        !ab_resp.variant_id.is_empty(),
        "AB experiment should assign"
    );
    assert!(
        !session_resp.variant_id.is_empty(),
        "session experiment should assign"
    );
}

#[tokio::test]
async fn layer_exclusive_allocation() {
    // Two experiments in the same layer with non-overlapping allocations.
    // A user should match at most one.
    let json = r#"{
        "experiments": [
            {
                "experiment_id": "exp_a",
                "state": "RUNNING",
                "hash_salt": "shared_salt",
                "layer_id": "layer_exclusive",
                "variants": [
                    { "variant_id": "a_ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
                ],
                "allocation": { "start_bucket": 0, "end_bucket": 4999 }
            },
            {
                "experiment_id": "exp_b",
                "state": "RUNNING",
                "hash_salt": "shared_salt",
                "layer_id": "layer_exclusive",
                "variants": [
                    { "variant_id": "b_ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
                ],
                "allocation": { "start_bucket": 5000, "end_bucket": 9999 }
            }
        ],
        "layers": [{ "layer_id": "layer_exclusive", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    for i in 0..500 {
        let user = format!("excl_user_{i}");
        let a = svc.assign("exp_a", &user, "", &no_attrs()).await.unwrap();
        let b = svc.assign("exp_b", &user, "", &no_attrs()).await.unwrap();

        // With same salt and same layer, user hashes to one bucket.
        // That bucket is in exactly one allocation range.
        let a_assigned = !a.variant_id.is_empty();
        let b_assigned = !b.variant_id.is_empty();
        assert!(
            !(a_assigned && b_assigned),
            "user {user} matched both exp_a and exp_b — allocations must be exclusive"
        );
        assert!(
            a_assigned || b_assigned,
            "user {user} matched neither — should match exactly one"
        );
    }
}

// ── M2.7 Tests (GetInterleavedList / Team Draft) ──

fn make_algo_lists(a_items: &[&str], b_items: &[&str]) -> HashMap<String, RankedList> {
    let mut m = HashMap::new();
    m.insert(
        "algo_a".to_string(),
        RankedList {
            item_ids: a_items.iter().map(|s| s.to_string()).collect(),
        },
    );
    m.insert(
        "algo_b".to_string(),
        RankedList {
            item_ids: b_items.iter().map(|s| s.to_string()).collect(),
        },
    );
    m
}

#[test]
fn interleaving_basic() {
    let svc = make_service();
    let lists = make_algo_lists(
        &["i1", "i2", "i3", "i4", "i5"],
        &["i6", "i7", "i8", "i9", "i10"],
    );
    let resp = svc.interleave("exp_dev_004", "user_1", &lists).unwrap();

    assert!(
        !resp.merged_list.is_empty(),
        "merged list should not be empty"
    );
    assert!(
        resp.merged_list.len() <= 10,
        "should not exceed max_list_size"
    );
    // Every item has provenance.
    for item in &resp.merged_list {
        assert!(
            resp.provenance.contains_key(item),
            "missing provenance for {item}"
        );
        let algo = &resp.provenance[item];
        assert!(
            algo == "algo_a" || algo == "algo_b",
            "unexpected provenance: {algo}"
        );
    }
}

#[test]
fn interleaving_deterministic() {
    let svc = make_service();
    let lists = make_algo_lists(&["x1", "x2", "x3", "x4"], &["y1", "y2", "y3", "y4"]);
    let r1 = svc.interleave("exp_dev_004", "user_42", &lists).unwrap();
    let r2 = svc.interleave("exp_dev_004", "user_42", &lists).unwrap();
    assert_eq!(
        r1.merged_list, r2.merged_list,
        "same inputs must produce same output"
    );
    assert_eq!(r1.provenance, r2.provenance);
}

#[test]
fn interleaving_respects_max_list_size() {
    // exp_dev_004 has max_list_size=10. Provide 20 items total.
    let svc = make_service();
    let a: Vec<&str> = (0..10)
        .map(|i| ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9"][i])
        .collect();
    let b: Vec<&str> = (0..10)
        .map(|i| ["b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8", "b9"][i])
        .collect();
    let lists = make_algo_lists(&a, &b);
    let resp = svc.interleave("exp_dev_004", "user_1", &lists).unwrap();
    assert!(
        resp.merged_list.len() <= 10,
        "merged list length {} exceeds max_list_size 10",
        resp.merged_list.len(),
    );
}

#[test]
fn interleaving_unknown_experiment() {
    let svc = make_service();
    let lists = make_algo_lists(&["a"], &["b"]);
    let err = svc
        .interleave("nonexistent_exp", "user_1", &lists)
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[test]
fn interleaving_missing_config() {
    // exp_dev_001 is an AB experiment with no interleaving_config.
    let svc = make_service();
    let lists = make_algo_lists(&["a"], &["b"]);
    let err = svc.interleave("exp_dev_001", "user_1", &lists).unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}

#[test]
fn interleaving_wrong_algo_count() {
    let svc = make_service();

    // 1 algorithm
    let mut one = HashMap::new();
    one.insert(
        "algo_a".to_string(),
        RankedList {
            item_ids: vec!["x".to_string()],
        },
    );
    let err = svc.interleave("exp_dev_004", "user_1", &one).unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    // 3 algorithms
    let mut three = HashMap::new();
    three.insert(
        "algo_a".to_string(),
        RankedList {
            item_ids: vec!["x".to_string()],
        },
    );
    three.insert(
        "algo_b".to_string(),
        RankedList {
            item_ids: vec!["y".to_string()],
        },
    );
    three.insert(
        "algo_c".to_string(),
        RankedList {
            item_ids: vec!["z".to_string()],
        },
    );
    let err = svc.interleave("exp_dev_004", "user_1", &three).unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

#[test]
fn interleaving_empty_lists() {
    let svc = make_service();
    let lists = make_algo_lists(&[], &[]);
    let resp = svc.interleave("exp_dev_004", "user_1", &lists).unwrap();
    assert!(resp.merged_list.is_empty(), "both empty → empty merged");
    assert!(resp.provenance.is_empty());
}

#[test]
fn interleaving_disjoint_lists() {
    let svc = make_service();
    let lists = make_algo_lists(&["a", "b", "c"], &["d", "e", "f"]);
    let resp = svc.interleave("exp_dev_004", "user_1", &lists).unwrap();
    // All 6 items should appear.
    assert_eq!(
        resp.merged_list.len(),
        6,
        "all disjoint items should appear"
    );
    let set: std::collections::HashSet<_> = resp.merged_list.iter().collect();
    for item in &["a", "b", "c", "d", "e", "f"] {
        assert!(set.contains(&item.to_string()), "missing item {item}");
    }
}

#[test]
fn interleaving_duplicate_items() {
    // Both lists share some items — should be deduped in merged output.
    let svc = make_service();
    let lists = make_algo_lists(&["shared", "a_only"], &["shared", "b_only"]);
    let resp = svc.interleave("exp_dev_004", "user_1", &lists).unwrap();

    let count_shared = resp.merged_list.iter().filter(|i| *i == "shared").count();
    assert_eq!(count_shared, 1, "shared item should appear exactly once");

    // "shared" should have provenance from whichever team picked first.
    assert!(resp.provenance.contains_key("shared"));
    // All unique items present.
    assert_eq!(resp.merged_list.len(), 3);
}

#[test]
fn interleaving_balance() {
    // Over many users, both algorithms should contribute ~equally.
    let svc = make_service();
    let mut algo_a_count = 0u64;
    let mut algo_b_count = 0u64;

    for i in 0..1000 {
        let user_id = format!("balance_user_{i}");
        let lists = make_algo_lists(&["i1", "i2", "i3", "i4"], &["i5", "i6", "i7", "i8"]);
        let resp = svc.interleave("exp_dev_004", &user_id, &lists).unwrap();
        for algo in resp.provenance.values() {
            match algo.as_str() {
                "algo_a" => algo_a_count += 1,
                "algo_b" => algo_b_count += 1,
                _ => panic!("unexpected algo: {algo}"),
            }
        }
    }

    let total = (algo_a_count + algo_b_count) as f64;
    let frac_a = algo_a_count as f64 / total;
    assert!(
        (0.40..=0.60).contains(&frac_a),
        "algo_a fraction {frac_a:.3} is outside [0.40, 0.60] — balance is off"
    );
}

// ── M2.7b Tests (GetInterleavedList / Optimized Interleaving) ──

#[test]
fn optimized_interleaving_basic() {
    let svc = make_service();
    let lists = make_algo_lists(
        &["i1", "i2", "i3", "i4", "i5"],
        &["i6", "i7", "i8", "i9", "i10"],
    );
    let resp = svc.interleave("exp_dev_006", "user_1", &lists).unwrap();

    assert!(!resp.merged_list.is_empty());
    assert!(resp.merged_list.len() <= 10);
    for item in &resp.merged_list {
        assert!(resp.provenance.contains_key(item));
        let algo = &resp.provenance[item];
        assert!(algo == "algo_a" || algo == "algo_b");
    }
}

#[test]
fn optimized_interleaving_deterministic() {
    let svc = make_service();
    let lists = make_algo_lists(&["x1", "x2", "x3", "x4"], &["y1", "y2", "y3", "y4"]);
    let r1 = svc.interleave("exp_dev_006", "user_42", &lists).unwrap();
    let r2 = svc.interleave("exp_dev_006", "user_42", &lists).unwrap();
    assert_eq!(r1.merged_list, r2.merged_list);
    assert_eq!(r1.provenance, r2.provenance);
}

#[test]
fn optimized_interleaving_respects_max_list_size() {
    let svc = make_service();
    let a: Vec<&str> = (0..10)
        .map(|i| ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9"][i])
        .collect();
    let b: Vec<&str> = (0..10)
        .map(|i| ["b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8", "b9"][i])
        .collect();
    let lists = make_algo_lists(&a, &b);
    let resp = svc.interleave("exp_dev_006", "user_1", &lists).unwrap();
    assert!(
        resp.merged_list.len() <= 10,
        "merged list length {} exceeds max_list_size 10",
        resp.merged_list.len(),
    );
}

#[test]
fn optimized_interleaving_empty_lists() {
    let svc = make_service();
    let lists = make_algo_lists(&[], &[]);
    let resp = svc.interleave("exp_dev_006", "user_1", &lists).unwrap();
    assert!(resp.merged_list.is_empty());
    assert!(resp.provenance.is_empty());
}

#[test]
fn optimized_interleaving_dedup() {
    let svc = make_service();
    let lists = make_algo_lists(&["shared", "a_only"], &["shared", "b_only"]);
    let resp = svc.interleave("exp_dev_006", "user_1", &lists).unwrap();

    let count_shared = resp.merged_list.iter().filter(|i| *i == "shared").count();
    assert_eq!(count_shared, 1, "shared item should appear exactly once");
    assert_eq!(resp.merged_list.len(), 3);
}

#[test]
fn optimized_interleaving_balance() {
    let svc = make_service();
    let mut algo_a_count = 0u64;
    let mut algo_b_count = 0u64;

    for i in 0..1000 {
        let user_id = format!("opt_balance_user_{i}");
        let lists = make_algo_lists(&["i1", "i2", "i3", "i4"], &["i5", "i6", "i7", "i8"]);
        let resp = svc.interleave("exp_dev_006", &user_id, &lists).unwrap();
        for algo in resp.provenance.values() {
            match algo.as_str() {
                "algo_a" => algo_a_count += 1,
                "algo_b" => algo_b_count += 1,
                _ => panic!("unexpected algo: {algo}"),
            }
        }
    }

    let total = (algo_a_count + algo_b_count) as f64;
    let frac_a = algo_a_count as f64 / total;
    assert!(
        (0.35..=0.65).contains(&frac_a),
        "algo_a fraction {frac_a:.3} is outside [0.35, 0.65]"
    );
}

#[test]
fn unsupported_method_returns_error() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "bad_method_exp",
            "state": "RUNNING",
            "type": "INTERLEAVING",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "algo_a", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}" },
                { "variant_id": "algo_b", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "interleaving_config": {
                "method": "NONEXISTENT",
                "algorithm_ids": ["algo_a", "algo_b"],
                "max_list_size": 10
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let lists = make_algo_lists(&["a", "b"], &["c", "d"]);
    let err = svc
        .interleave("bad_method_exp", "user_1", &lists)
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("unsupported interleaving method"));
}

// ── M3.x Tests (GetInterleavedList / Multileave) ──

fn make_multi_algo_lists(items: &[(&str, &[&str])]) -> HashMap<String, RankedList> {
    items
        .iter()
        .map(|(algo_id, item_ids)| {
            (
                algo_id.to_string(),
                RankedList {
                    item_ids: item_ids.iter().map(|s| s.to_string()).collect(),
                },
            )
        })
        .collect()
}

#[test]
fn multileave_basic_3_algorithms() {
    let svc = make_service();
    let lists = make_multi_algo_lists(&[
        ("algo_x", &["i1", "i2", "i3", "i4", "i5"]),
        ("algo_y", &["i6", "i7", "i8", "i9", "i10"]),
        ("algo_z", &["i11", "i12", "i13", "i14", "i15"]),
    ]);
    let resp = svc.interleave("exp_dev_007", "user_1", &lists).unwrap();

    assert!(!resp.merged_list.is_empty());
    assert!(resp.merged_list.len() <= 15);
    for item in &resp.merged_list {
        assert!(
            resp.provenance.contains_key(item),
            "missing provenance for {item}"
        );
        let algo = &resp.provenance[item];
        assert!(
            ["algo_x", "algo_y", "algo_z"].contains(&algo.as_str()),
            "unexpected provenance: {algo}"
        );
    }
}

#[test]
fn multileave_deterministic() {
    let svc = make_service();
    let lists = make_multi_algo_lists(&[
        ("algo_x", &["x1", "x2", "x3"]),
        ("algo_y", &["y1", "y2", "y3"]),
        ("algo_z", &["z1", "z2", "z3"]),
    ]);
    let r1 = svc.interleave("exp_dev_007", "user_42", &lists).unwrap();
    let r2 = svc.interleave("exp_dev_007", "user_42", &lists).unwrap();
    assert_eq!(
        r1.merged_list, r2.merged_list,
        "same inputs must produce same output"
    );
    assert_eq!(r1.provenance, r2.provenance);
}

#[test]
fn multileave_balance_3_algorithms() {
    let svc = make_service();
    let mut counts: HashMap<String, u64> = HashMap::new();

    for i in 0..1000 {
        let user_id = format!("ml_balance_user_{i}");
        let lists = make_multi_algo_lists(&[
            ("algo_x", &["i1", "i2", "i3", "i4"]),
            ("algo_y", &["i5", "i6", "i7", "i8"]),
            ("algo_z", &["i9", "i10", "i11", "i12"]),
        ]);
        let resp = svc.interleave("exp_dev_007", &user_id, &lists).unwrap();
        for algo in resp.provenance.values() {
            *counts.entry(algo.clone()).or_insert(0) += 1;
        }
    }

    let total: u64 = counts.values().sum();
    for (algo, count) in &counts {
        let frac = *count as f64 / total as f64;
        assert!(
            (0.25..=0.42).contains(&frac),
            "algo {algo} fraction {frac:.3} outside [0.25, 0.42]"
        );
    }
}

#[test]
fn multileave_dedup_across_3_lists() {
    let svc = make_service();
    let lists = make_multi_algo_lists(&[
        ("algo_x", &["shared", "x_only"]),
        ("algo_y", &["shared", "y_only"]),
        ("algo_z", &["shared", "z_only"]),
    ]);
    let resp = svc.interleave("exp_dev_007", "user_1", &lists).unwrap();

    let count_shared = resp.merged_list.iter().filter(|i| *i == "shared").count();
    assert_eq!(count_shared, 1, "shared item should appear exactly once");
    assert_eq!(resp.merged_list.len(), 4); // shared + x_only + y_only + z_only
}

#[test]
fn multileave_respects_max_list_size() {
    // exp_dev_007 has max_list_size=15. Provide 30 items total.
    let svc = make_service();
    let lists = make_multi_algo_lists(&[
        (
            "algo_x",
            &["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9"],
        ),
        (
            "algo_y",
            &["b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8", "b9"],
        ),
        (
            "algo_z",
            &["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9"],
        ),
    ]);
    let resp = svc.interleave("exp_dev_007", "user_1", &lists).unwrap();
    assert!(
        resp.merged_list.len() <= 15,
        "merged list length {} exceeds max_list_size 15",
        resp.merged_list.len(),
    );
}

#[test]
fn multileave_empty_lists() {
    let svc = make_service();
    let lists = make_multi_algo_lists(&[("algo_x", &[]), ("algo_y", &[]), ("algo_z", &[])]);
    let resp = svc.interleave("exp_dev_007", "user_1", &lists).unwrap();
    assert!(resp.merged_list.is_empty());
    assert!(resp.provenance.is_empty());
}

#[test]
fn multileave_wrong_algo_count_for_multileave() {
    // MULTILEAVE config with only 2 algorithm_ids → FailedPrecondition.
    let json = r#"{
        "experiments": [{
            "experiment_id": "bad_multi",
            "state": "RUNNING",
            "type": "INTERLEAVING",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "a", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}" },
                { "variant_id": "b", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "interleaving_config": {
                "method": "MULTILEAVE",
                "algorithm_ids": ["a", "b"],
                "max_list_size": 10
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let lists = make_algo_lists(&["x", "y"], &["z", "w"]);
    let err = svc.interleave("bad_multi", "user_1", &lists).unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("MULTILEAVE requires >= 3"));
}

#[test]
fn multileave_missing_algo_list_in_request() {
    // Config has 3 algorithm_ids but request only provides 2 lists.
    let svc = make_service();
    let lists = make_multi_algo_lists(&[
        ("algo_x", &["i1", "i2"]),
        ("algo_y", &["i3", "i4"]),
        // algo_z is missing
    ]);
    let err = svc.interleave("exp_dev_007", "user_1", &lists).unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("algo_z"));
}

// ── Bandit Delegation Tests (MAB / CONTEXTUAL_BANDIT) ──

#[tokio::test]
async fn bandit_mab_basic_assignment() {
    let svc = make_service();
    let resp = svc
        .assign("exp_dev_005", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert!(
        !resp.variant_id.is_empty(),
        "MAB experiment should assign an arm"
    );
    // Arm should be one of the configured arms.
    assert!(
        ["arm_hero", "arm_carousel", "arm_spotlight"].contains(&resp.variant_id.as_str()),
        "unexpected arm: {}",
        resp.variant_id
    );
    // Assignment probability should be 1/3 (uniform mock).
    assert!(
        (resp.assignment_probability - 1.0 / 3.0).abs() < 1e-9,
        "expected ~0.333, got {}",
        resp.assignment_probability
    );
}

#[tokio::test]
async fn bandit_mab_deterministic() {
    let svc = make_service();
    let r1 = svc
        .assign("exp_dev_005", "user_42", "", &no_attrs())
        .await
        .unwrap();
    let r2 = svc
        .assign("exp_dev_005", "user_42", "", &no_attrs())
        .await
        .unwrap();
    assert_eq!(r1.variant_id, r2.variant_id, "same user must get same arm");
    assert!((r1.assignment_probability - r2.assignment_probability).abs() < f64::EPSILON);
}

#[tokio::test]
async fn bandit_mab_balance() {
    let svc = make_service();
    let mut counts: HashMap<String, u64> = HashMap::new();

    for i in 0..3000 {
        let user = format!("bandit_user_{i}");
        let resp = svc
            .assign("exp_dev_005", &user, "", &no_attrs())
            .await
            .unwrap();
        *counts.entry(resp.variant_id).or_insert(0) += 1;
    }

    // Each of 3 arms should get ~1000 ± 150 (uniform).
    assert_eq!(counts.len(), 3, "should see all 3 arms");
    for (arm, count) in &counts {
        let frac = *count as f64 / 3000.0;
        assert!(
            (0.25..=0.42).contains(&frac),
            "arm {arm} fraction {frac:.3} outside [0.25, 0.42]"
        );
    }
}

#[tokio::test]
async fn bandit_mab_payload_propagated() {
    let svc = make_service();
    let resp = svc
        .assign("exp_dev_005", "user_1", "", &no_attrs())
        .await
        .unwrap();
    // Payload should contain the arm's payload_json from bandit_config.
    assert!(
        resp.payload_json.contains("placement"),
        "payload should contain placement config: {}",
        resp.payload_json
    );
}

#[tokio::test]
async fn bandit_missing_config_fails() {
    // AB experiment used as MAB type — no bandit_config.
    let json = r#"{
        "experiments": [{
            "experiment_id": "bad_mab",
            "state": "RUNNING",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let err = svc
        .assign("bad_mab", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("bandit_config"));
}

#[tokio::test]
async fn bandit_empty_arms_fails() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "empty_arms_mab",
            "state": "RUNNING",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "ctrl", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [],
                "reward_metric_id": "clicks"
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let err = svc
        .assign("empty_arms_mab", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("no arms"));
}

#[tokio::test]
async fn bandit_contextual_type_also_delegates() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "ctx_bandit",
            "state": "RUNNING",
            "type": "CONTEXTUAL_BANDIT",
            "hash_salt": "ctx_salt",
            "layer_id": "layer_default",
            "variants": [
                { "variant_id": "arm_a", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}" },
                { "variant_id": "arm_b", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{}" }
            ],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "LINEAR_UCB",
                "arms": [
                    { "arm_id": "arm_a", "name": "Arm A", "payload_json": "{\"a\":1}" },
                    { "arm_id": "arm_b", "name": "Arm B", "payload_json": "{\"b\":2}" }
                ],
                "reward_metric_id": "engagement",
                "context_feature_keys": ["age", "tenure"]
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let resp = svc
        .assign("ctx_bandit", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert!(
        resp.variant_id == "arm_a" || resp.variant_id == "arm_b",
        "unexpected arm: {}",
        resp.variant_id
    );
    // 2 arms → 0.5 probability each.
    assert!(
        (resp.assignment_probability - 0.5).abs() < 1e-9,
        "expected 0.5, got {}",
        resp.assignment_probability
    );
}

#[tokio::test]
async fn bandit_not_running_returns_inactive() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "draft_mab",
            "state": "DRAFT",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [
                    { "arm_id": "arm_1", "name": "Arm 1", "payload_json": "{}" }
                ],
                "reward_metric_id": "clicks"
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    let resp = svc
        .assign("draft_mab", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(!resp.is_active, "DRAFT MAB should return is_active=false");
    assert!(resp.variant_id.is_empty());
}

#[tokio::test]
async fn bandit_targeting_still_applies() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "targeted_mab",
            "state": "RUNNING",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "targeting_rule": {
                "groups": [{
                    "predicates": [{ "attribute_key": "country", "operator": "IN", "values": ["US"] }]
                }]
            },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [
                    { "arm_id": "arm_1", "name": "Arm 1", "payload_json": "{}" },
                    { "arm_id": "arm_2", "name": "Arm 2", "payload_json": "{}" }
                ],
                "reward_metric_id": "clicks"
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    // User with US country → should get arm
    let resp = svc
        .assign("targeted_mab", "user_1", "", &attrs(&[("country", "US")]))
        .await
        .unwrap();
    assert!(!resp.variant_id.is_empty(), "US user should get an arm");

    // User with FR country → targeting miss
    let resp2 = svc
        .assign("targeted_mab", "user_2", "", &attrs(&[("country", "FR")]))
        .await
        .unwrap();
    assert!(
        resp2.variant_id.is_empty(),
        "FR user should not match targeting"
    );
}

// ── Live Bandit gRPC Integration Tests ──

// ── Session-Level Locked Variant Tests (allow_cross_session_variation=false) ──

#[tokio::test]
async fn session_level_locked_variant_same_across_sessions() {
    let svc = make_service();
    let user_id = "locked_user_42";

    // Assign with 50 different session_ids — all should return the same variant.
    let first = svc
        .assign("exp_dev_008", user_id, "session_0", &no_attrs())
        .await
        .unwrap();
    assert!(first.is_active);
    assert!(!first.variant_id.is_empty());

    for i in 1..50 {
        let session = format!("session_{i}");
        let resp = svc
            .assign("exp_dev_008", user_id, &session, &no_attrs())
            .await
            .unwrap();
        assert_eq!(
            resp.variant_id, first.variant_id,
            "locked variant must be the same across sessions (session_{i} got {} vs {})",
            resp.variant_id, first.variant_id,
        );
    }
}

#[tokio::test]
async fn session_level_locked_variant_different_users() {
    let svc = make_service();
    let session_id = "shared_session";

    let mut variants = std::collections::HashSet::new();
    for i in 0..100 {
        let user = format!("locked_user_{i}");
        let resp = svc
            .assign("exp_dev_008", &user, session_id, &no_attrs())
            .await
            .unwrap();
        assert!(resp.is_active);
        variants.insert(resp.variant_id);
    }

    assert!(
        variants.len() >= 2,
        "100 different users should produce at least 2 distinct variants, got {:?}",
        variants,
    );
}

#[tokio::test]
async fn session_level_locked_still_requires_session_id() {
    let svc = make_service();
    let err = svc
        .assign("exp_dev_008", "user_1", "", &no_attrs())
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("session_id"));
}

/// Mock M4b BanditPolicyService that returns a fixed arm selection.
struct MockBanditService {
    /// If Some, sleep this long before responding (to test timeout).
    delay: Option<Duration>,
    /// The arm_id to return.
    arm_id: String,
    /// The probability to return.
    probability: f64,
    /// Captured context features (for verification in tests).
    captured_features: Arc<tokio::sync::Mutex<Option<HashMap<String, f64>>>>,
}

#[tonic::async_trait]
impl BanditPolicyService for MockBanditService {
    async fn select_arm(
        &self,
        request: tonic::Request<SelectArmRequest>,
    ) -> Result<tonic::Response<ArmSelection>, tonic::Status> {
        let req = request.into_inner();

        // Capture context features for test verification.
        *self.captured_features.lock().await = Some(req.context_features.clone());

        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }

        let mut all_probs = HashMap::new();
        all_probs.insert(self.arm_id.clone(), self.probability);

        Ok(tonic::Response::new(ArmSelection {
            arm_id: self.arm_id.clone(),
            assignment_probability: self.probability,
            all_arm_probabilities: all_probs,
        }))
    }

    async fn get_slate_assignment(
        &self,
        _request: tonic::Request<SlateAssignmentRequest>,
    ) -> Result<tonic::Response<SlateAssignmentResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed for assignment_test mock"))
    }

    async fn create_cold_start_bandit(
        &self,
        request: tonic::Request<CreateColdStartBanditRequest>,
    ) -> Result<tonic::Response<CreateColdStartBanditResponse>, tonic::Status> {
        let req = request.into_inner();
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        Ok(tonic::Response::new(CreateColdStartBanditResponse {
            experiment_id: format!("cold-start:{}", req.content_id),
            content_id: req.content_id,
        }))
    }

    async fn export_affinity_scores(
        &self,
        request: tonic::Request<ExportAffinityScoresRequest>,
    ) -> Result<tonic::Response<ExportAffinityScoresResponse>, tonic::Status> {
        let req = request.into_inner();
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        let mut scores = HashMap::new();
        scores.insert("teens".to_string(), 0.85);
        scores.insert("adults".to_string(), 0.62);
        let mut placements = HashMap::new();
        placements.insert("teens".to_string(), "arm_prominent".to_string());
        placements.insert("adults".to_string(), "arm_carousel".to_string());
        Ok(tonic::Response::new(ExportAffinityScoresResponse {
            content_id: format!("content-for-{}", req.experiment_id),
            segment_affinity_scores: scores,
            optimal_placements: placements,
        }))
    }

    async fn get_policy_snapshot(
        &self,
        _request: tonic::Request<GetPolicySnapshotRequest>,
    ) -> Result<tonic::Response<PolicySnapshot>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed for test"))
    }

    async fn rollback_policy(
        &self,
        _request: tonic::Request<RollbackPolicyRequest>,
    ) -> Result<tonic::Response<PolicySnapshot>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed for test"))
    }
}

/// Start a mock M4b server on a random port, return the address.
async fn start_mock_m4b(
    delay: Option<Duration>,
    arm_id: &str,
    probability: f64,
) -> (
    SocketAddr,
    Arc<tokio::sync::Mutex<Option<HashMap<String, f64>>>>,
) {
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let svc = MockBanditService {
        delay,
        arm_id: arm_id.to_string(),
        probability,
        captured_features: captured.clone(),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(BanditPolicyServiceServer::new(svc))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    // Give server a moment to start.
    tokio::time::sleep(Duration::from_millis(50)).await;

    (addr, captured)
}

#[tokio::test]
async fn bandit_grpc_client_success() {
    // Start mock M4b that returns arm_hero with 0.7 probability.
    let (addr, _captured) = start_mock_m4b(None, "arm_hero", 0.7).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    // Build service with MAB experiment config that has arm_hero.
    let json = r#"{
        "experiments": [{
            "experiment_id": "live_mab",
            "state": "RUNNING",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [
                    { "arm_id": "arm_hero", "name": "Hero", "payload_json": "{\"placement\":\"hero\"}" },
                    { "arm_id": "arm_carousel", "name": "Carousel", "payload_json": "{\"placement\":\"carousel\"}" }
                ],
                "reward_metric_id": "clicks"
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::new(
        experimentation_assignment::config_cache::ConfigCacheHandle::from_static(Arc::new(config)),
        Some(client),
    );

    let resp = svc
        .assign("live_mab", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert_eq!(resp.variant_id, "arm_hero");
    assert!((resp.assignment_probability - 0.7).abs() < 1e-9);
    // Payload comes from local config, not M4b.
    assert_eq!(resp.payload_json, r#"{"placement":"hero"}"#);
}

#[tokio::test]
async fn bandit_grpc_timeout_falls_back() {
    // Start mock M4b that sleeps 50ms (>> 10ms timeout).
    let (addr, _captured) = start_mock_m4b(Some(Duration::from_millis(50)), "arm_hero", 0.7).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    let json = r#"{
        "experiments": [{
            "experiment_id": "timeout_mab",
            "state": "RUNNING",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [
                    { "arm_id": "arm_a", "name": "A", "payload_json": "{\"a\":1}" },
                    { "arm_id": "arm_b", "name": "B", "payload_json": "{\"b\":2}" }
                ],
                "reward_metric_id": "clicks"
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::new(
        experimentation_assignment::config_cache::ConfigCacheHandle::from_static(Arc::new(config)),
        Some(client),
    );

    // Should fall back to uniform random (not fail).
    let resp = svc
        .assign("timeout_mab", "user_1", "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert!(
        resp.variant_id == "arm_a" || resp.variant_id == "arm_b",
        "fallback should return one of the arms, got: {}",
        resp.variant_id
    );
    // Uniform probability: 1/2 = 0.5.
    assert!(
        (resp.assignment_probability - 0.5).abs() < 1e-9,
        "fallback should use uniform probability, got {}",
        resp.assignment_probability
    );
}

#[tokio::test]
async fn bandit_contextual_features_forwarded() {
    // Start mock M4b that captures context features.
    let (addr, captured) = start_mock_m4b(None, "arm_a", 0.6).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    let json = r#"{
        "experiments": [{
            "experiment_id": "ctx_mab",
            "state": "RUNNING",
            "type": "CONTEXTUAL_BANDIT",
            "hash_salt": "salt",
            "layer_id": "layer_default",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "LINEAR_UCB",
                "arms": [
                    { "arm_id": "arm_a", "name": "A", "payload_json": "{}" },
                    { "arm_id": "arm_b", "name": "B", "payload_json": "{}" }
                ],
                "reward_metric_id": "engagement",
                "context_feature_keys": ["age", "tenure"]
            }
        }],
        "layers": [{ "layer_id": "layer_default", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let svc = AssignmentServiceImpl::new(
        experimentation_assignment::config_cache::ConfigCacheHandle::from_static(Arc::new(config)),
        Some(client),
    );

    let user_attrs = attrs(&[("age", "30"), ("tenure", "2.5"), ("country", "US")]);
    let resp = svc
        .assign("ctx_mab", "user_1", "", &user_attrs)
        .await
        .unwrap();
    assert!(resp.is_active);
    assert_eq!(resp.variant_id, "arm_a");

    // Verify context features were forwarded to M4b.
    let features = captured.lock().await.take().unwrap();
    assert_eq!(
        features.len(),
        2,
        "should forward 2 context features (age, tenure)"
    );
    assert!((features["age"] - 30.0).abs() < f64::EPSILON);
    assert!((features["tenure"] - 2.5).abs() < f64::EPSILON);
    // "country" should NOT be in features (not in context_feature_keys).
    assert!(!features.contains_key("country"));
}

// ── Cold-Start Bandit Tests ──

#[tokio::test]
async fn cold_start_create_success() {
    let (addr, _captured) = start_mock_m4b(None, "arm_a", 0.5).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    let mut metadata = HashMap::new();
    metadata.insert("genre".to_string(), "action".to_string());

    let result = client
        .create_cold_start_bandit("movie-new-001", metadata, 14)
        .await
        .unwrap();

    assert_eq!(result.experiment_id, "cold-start:movie-new-001");
    assert_eq!(result.content_id, "movie-new-001");
}

#[tokio::test]
async fn cold_start_create_timeout() {
    // Mock sleeps 6s, but cold-start timeout is 5s.
    let (addr, _captured) = start_mock_m4b(Some(Duration::from_secs(6)), "arm_a", 0.5).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    let result = client
        .create_cold_start_bandit("movie-timeout", HashMap::new(), 7)
        .await;

    assert!(result.is_err(), "cold-start should timeout after 5s");
}

#[tokio::test]
async fn export_affinity_scores_success() {
    let (addr, _captured) = start_mock_m4b(None, "arm_a", 0.5).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    let result = client
        .export_affinity_scores("cold-start:movie-001")
        .await
        .unwrap();

    assert_eq!(result.content_id, "content-for-cold-start:movie-001");
    assert_eq!(result.segment_affinity_scores.len(), 2);
    assert!((result.segment_affinity_scores["teens"] - 0.85).abs() < f64::EPSILON);
    assert!((result.segment_affinity_scores["adults"] - 0.62).abs() < f64::EPSILON);
    assert_eq!(result.optimal_placements["teens"], "arm_prominent");
    assert_eq!(result.optimal_placements["adults"], "arm_carousel");
}

#[tokio::test]
async fn cold_start_experiment_assignment() {
    // The cold-start experiment in dev/config.json should be assignable via
    // the existing SelectArm path (it's a CONTEXTUAL_BANDIT type).
    let (addr, captured) = start_mock_m4b(None, "arm_prominent", 0.6).await;

    let client = experimentation_assignment::bandit_client::GrpcBanditClient::connect(&format!(
        "http://{addr}"
    ))
    .await
    .unwrap();

    let config = Config::from_json(DEV_CONFIG).unwrap();
    let svc = AssignmentServiceImpl::new(
        experimentation_assignment::config_cache::ConfigCacheHandle::from_static(Arc::new(config)),
        Some(client),
    );

    let user_attrs = attrs(&[
        ("genre_affinity", "0.9"),
        ("recency_days", "3"),
        ("tenure_months", "12.5"),
    ]);

    let resp = svc
        .assign("cold-start:movie-new-001", "user_42", "", &user_attrs)
        .await
        .unwrap();

    assert!(resp.is_active);
    assert_eq!(resp.variant_id, "arm_prominent");
    assert!((resp.assignment_probability - 0.6).abs() < 1e-9);

    // Verify context features were forwarded for CONTEXTUAL_BANDIT.
    let features = captured.lock().await.take().unwrap();
    assert_eq!(features.len(), 3);
    assert!((features["genre_affinity"] - 0.9).abs() < f64::EPSILON);
    assert!((features["recency_days"] - 3.0).abs() < f64::EPSILON);
    assert!((features["tenure_months"] - 12.5).abs() < f64::EPSILON);
}

// ── Cold-Start Config Tests ──

#[tokio::test]
async fn config_cold_start_fields_parse() {
    let json = r#"{
        "experiments": [{
            "experiment_id": "cs_test",
            "state": "RUNNING",
            "type": "CONTEXTUAL_BANDIT",
            "hash_salt": "salt",
            "layer_id": "layer_1",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [{ "arm_id": "a1", "name": "A1", "payload_json": "{}" }],
                "reward_metric_id": "clicks",
                "content_id": "movie-42",
                "cold_start_window_days": 21
            }
        }],
        "layers": [{ "layer_id": "layer_1", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let exp = &config.experiments_by_id["cs_test"];
    let bandit = exp.bandit_config.as_ref().unwrap();
    assert_eq!(bandit.content_id.as_deref(), Some("movie-42"));
    assert_eq!(bandit.cold_start_window_days, Some(21));
}

#[tokio::test]
async fn config_cold_start_fields_default_to_none() {
    // Existing configs without cold-start fields should still parse.
    let json = r#"{
        "experiments": [{
            "experiment_id": "regular_mab",
            "state": "RUNNING",
            "type": "MAB",
            "hash_salt": "salt",
            "layer_id": "layer_1",
            "variants": [],
            "allocation": { "start_bucket": 0, "end_bucket": 9999 },
            "bandit_config": {
                "algorithm": "THOMPSON_SAMPLING",
                "arms": [{ "arm_id": "a1", "name": "A1", "payload_json": "{}" }],
                "reward_metric_id": "clicks"
            }
        }],
        "layers": [{ "layer_id": "layer_1", "total_buckets": 10000 }]
    }"#;

    let config = Config::from_json(json).unwrap();
    let bandit = config.experiments_by_id["regular_mab"]
        .bandit_config
        .as_ref()
        .unwrap();
    assert!(bandit.content_id.is_none());
    assert!(bandit.cold_start_window_days.is_none());
}

// ── Cumulative Holdout Priority Tests ──

/// Helper: build a config with a holdout and an AB experiment in the same layer.
fn make_holdout_config() -> Config {
    let json = r#"{
        "experiments": [
            {
                "experiment_id": "ab_exp",
                "state": "RUNNING",
                "type": "AB",
                "hash_salt": "ab_salt",
                "layer_id": "layer_shared",
                "variants": [
                    { "variant_id": "control", "traffic_fraction": 0.5, "is_control": true, "payload_json": "{}" },
                    { "variant_id": "treatment", "traffic_fraction": 0.5, "is_control": false, "payload_json": "{\"feature\": true}" }
                ],
                "allocation": { "start_bucket": 0, "end_bucket": 9999 }
            },
            {
                "experiment_id": "holdout_exp",
                "state": "RUNNING",
                "type": "CUMULATIVE_HOLDOUT",
                "hash_salt": "holdout_salt",
                "layer_id": "layer_shared",
                "is_cumulative_holdout": true,
                "variants": [
                    { "variant_id": "holdout", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{\"baseline\": true}" }
                ],
                "allocation": { "start_bucket": 0, "end_bucket": 499 }
            },
            {
                "experiment_id": "other_layer_exp",
                "state": "RUNNING",
                "type": "AB",
                "hash_salt": "other_salt",
                "layer_id": "layer_other",
                "variants": [
                    { "variant_id": "v1", "traffic_fraction": 1.0, "is_control": true, "payload_json": "{}" }
                ],
                "allocation": { "start_bucket": 0, "end_bucket": 9999 }
            }
        ],
        "layers": [
            { "layer_id": "layer_shared", "total_buckets": 10000 },
            { "layer_id": "layer_other", "total_buckets": 10000 }
        ]
    }"#;
    Config::from_json(json).unwrap()
}

#[tokio::test]
async fn holdout_priority_excludes_layer_experiments() {
    let config = make_holdout_config();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    // Find a user that lands in the holdout allocation (bucket 0–499).
    // With hash_salt "holdout_salt" and total_buckets 10000, iterate to find one.
    let mut holdout_user = None;
    for i in 0..200 {
        let uid = format!("holdout_test_user_{i}");
        let bucket = experimentation_hash::bucket(&uid, "holdout_salt", 10000);
        if bucket <= 499 {
            holdout_user = Some(uid);
            break;
        }
    }
    let user_id = holdout_user.expect("should find a user in holdout allocation within 200 tries");

    let req = tonic::Request::new(GetAssignmentsRequest {
        user_id: user_id.clone(),
        session_id: String::new(),
        attributes: HashMap::new(),
    });
    let resp = svc.get_assignments(req).await.unwrap().into_inner();

    // User should get the holdout assignment.
    let holdout_assignment = resp
        .assignments
        .iter()
        .find(|a| a.experiment_id == "holdout_exp");
    assert!(holdout_assignment.is_some(), "holdout assignment missing");
    assert_eq!(holdout_assignment.unwrap().variant_id, "holdout");

    // User should NOT get the AB experiment in the same layer.
    let ab_assignment = resp
        .assignments
        .iter()
        .find(|a| a.experiment_id == "ab_exp");
    assert!(
        ab_assignment.is_none(),
        "AB experiment in same layer should be excluded for holdout user"
    );

    // User should still get the experiment in a different layer.
    let other_assignment = resp
        .assignments
        .iter()
        .find(|a| a.experiment_id == "other_layer_exp");
    assert!(
        other_assignment.is_some(),
        "experiment in different layer should not be excluded"
    );
}

#[tokio::test]
async fn holdout_user_outside_allocation_gets_regular() {
    let config = make_holdout_config();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    // Find a user outside the holdout allocation (bucket > 499).
    let mut non_holdout_user = None;
    for i in 0..200 {
        let uid = format!("non_holdout_user_{i}");
        let bucket = experimentation_hash::bucket(&uid, "holdout_salt", 10000);
        if bucket > 499 {
            non_holdout_user = Some(uid);
            break;
        }
    }
    let user_id =
        non_holdout_user.expect("should find a user outside holdout allocation within 200 tries");

    let req = tonic::Request::new(GetAssignmentsRequest {
        user_id: user_id.clone(),
        session_id: String::new(),
        attributes: HashMap::new(),
    });
    let resp = svc.get_assignments(req).await.unwrap().into_inner();

    // Holdout assignment should exist but with empty variant (not in allocation).
    let holdout_assignment = resp
        .assignments
        .iter()
        .find(|a| a.experiment_id == "holdout_exp");
    assert!(holdout_assignment.is_some());
    assert!(
        holdout_assignment.unwrap().variant_id.is_empty(),
        "user outside holdout allocation should get empty variant_id"
    );

    // AB experiment should be present (holdout didn't claim the layer).
    let ab_assignment = resp
        .assignments
        .iter()
        .find(|a| a.experiment_id == "ab_exp");
    assert!(
        ab_assignment.is_some(),
        "AB experiment should be included when user is outside holdout"
    );
}

#[tokio::test]
async fn holdout_different_layer_no_exclusion() {
    let config = make_holdout_config();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    // Find a holdout user.
    let mut holdout_user = None;
    for i in 0..200 {
        let uid = format!("layer_test_user_{i}");
        let bucket = experimentation_hash::bucket(&uid, "holdout_salt", 10000);
        if bucket <= 499 {
            holdout_user = Some(uid);
            break;
        }
    }
    let user_id = holdout_user.expect("should find a holdout user");

    let req = tonic::Request::new(GetAssignmentsRequest {
        user_id: user_id.clone(),
        session_id: String::new(),
        attributes: HashMap::new(),
    });
    let resp = svc.get_assignments(req).await.unwrap().into_inner();

    // other_layer_exp is in layer_other — should NOT be excluded.
    let other_assignment = resp
        .assignments
        .iter()
        .find(|a| a.experiment_id == "other_layer_exp");
    assert!(
        other_assignment.is_some(),
        "holdout in layer_shared must not block experiments in layer_other"
    );
    assert!(
        !other_assignment.unwrap().variant_id.is_empty(),
        "other layer experiment should assign a variant"
    );
}

#[tokio::test]
async fn holdout_single_assign_works() {
    let config = make_holdout_config();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    // Single GetAssignment for the holdout — should work like any static experiment.
    let mut holdout_user = None;
    for i in 0..200 {
        let uid = format!("single_assign_user_{i}");
        let bucket = experimentation_hash::bucket(&uid, "holdout_salt", 10000);
        if bucket <= 499 {
            holdout_user = Some(uid);
            break;
        }
    }
    let user_id = holdout_user.expect("should find a holdout user");

    let resp = svc
        .assign("holdout_exp", &user_id, "", &no_attrs())
        .await
        .unwrap();
    assert!(resp.is_active);
    assert_eq!(resp.variant_id, "holdout");
    assert_eq!(resp.experiment_id, "holdout_exp");
}

#[tokio::test]
async fn holdout_deterministic() {
    let config = make_holdout_config();
    let svc = AssignmentServiceImpl::from_config(Arc::new(config));

    // Same user always gets same holdout result.
    let user_id = "deterministic_holdout_user_42";
    let r1 = svc
        .assign("holdout_exp", user_id, "", &no_attrs())
        .await
        .unwrap();
    let r2 = svc
        .assign("holdout_exp", user_id, "", &no_attrs())
        .await
        .unwrap();
    assert_eq!(r1.variant_id, r2.variant_id);
    assert_eq!(r1.is_active, r2.is_active);
    assert_eq!(r1.experiment_id, r2.experiment_id);
}
