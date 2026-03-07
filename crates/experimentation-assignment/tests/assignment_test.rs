use std::collections::HashMap;
use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;
use experimentation_proto::experimentation::assignment::v1::RankedList;

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
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ── M1.2 Tests (static bucketing) ──

#[test]
fn determinism_same_user_same_variant() {
    let svc = make_service();
    let r1 = svc.assign("exp_dev_001", "user_42", "", &no_attrs()).unwrap();
    let r2 = svc.assign("exp_dev_001", "user_42", "", &no_attrs()).unwrap();
    assert_eq!(r1.variant_id, r2.variant_id, "same user must get same variant");
    assert_eq!(r1.payload_json, r2.payload_json);
    assert!(r1.is_active);
}

#[test]
fn balance_50_50_chi_squared() {
    let svc = make_service();
    let mut counts = std::collections::HashMap::new();

    for i in 0..10_000 {
        let user_id = format!("balance_user_{i}");
        let resp = svc.assign("exp_dev_001", &user_id, "", &no_attrs()).unwrap();
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

#[test]
fn not_found_unknown_experiment() {
    let svc = make_service();
    let err = svc.assign("nonexistent_exp", "user_1", "", &no_attrs()).unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[test]
fn empty_assignment_outside_allocation() {
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
        let resp = svc.assign("narrow_exp", &format!("user_{i}"), "", &no_attrs()).unwrap();
        if resp.variant_id.is_empty() {
            assert!(resp.is_active, "outside allocation should still be is_active=true");
            outside_count += 1;
        }
    }

    assert!(
        outside_count > 900,
        "expected most users outside narrow allocation, got {outside_count}/1000 outside"
    );
}

#[test]
fn inactive_experiment_returns_is_active_false() {
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

    let resp = svc.assign("draft_exp", "user_1", "", &no_attrs()).unwrap();
    assert!(!resp.is_active, "DRAFT experiment must return is_active=false");
    assert!(resp.variant_id.is_empty(), "DRAFT experiment must not assign a variant");
}

// ── M1.4 Tests (targeting rules) ──

#[test]
fn targeting_country_in_match() {
    let svc = make_service();
    // exp_dev_002 targets country IN [US, UK] AND tier IN [premium, platinum]
    let resp = svc.assign("exp_dev_002", "user_1", "", &attrs(&[("country", "US"), ("tier", "premium")])).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty(), "targeted user should get a variant");
}

#[test]
fn targeting_empty_rule_matches_all() {
    let svc = make_service();
    // exp_dev_001 has no targeting rule — all users match
    let resp = svc.assign("exp_dev_001", "user_1", "", &no_attrs()).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty());
}

#[test]
fn targeting_missing_attribute_no_match() {
    let svc = make_service();
    // exp_dev_002 requires country + tier, but we provide neither
    let resp = svc.assign("exp_dev_002", "user_1", "", &no_attrs()).unwrap();
    assert!(resp.is_active, "targeting miss should still be is_active=true");
    assert!(resp.variant_id.is_empty(), "targeting miss should return empty variant");
}

#[test]
fn targeting_no_rule_matches_all() {
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

    let resp = svc.assign("no_target_exp", "user_1", "", &no_attrs()).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty(), "no targeting rule → all users match");
}

#[test]
fn targeting_compound_and_across_groups() {
    let svc = make_service();
    // Correct country but wrong tier → AND fails
    let resp = svc.assign("exp_dev_002", "user_1", "", &attrs(&[("country", "US"), ("tier", "free")])).unwrap();
    assert!(resp.variant_id.is_empty(), "wrong tier should not match");

    // Correct tier but wrong country → AND fails
    let resp2 = svc.assign("exp_dev_002", "user_2", "", &attrs(&[("country", "FR"), ("tier", "premium")])).unwrap();
    assert!(resp2.variant_id.is_empty(), "wrong country should not match");
}

#[test]
fn targeting_gt_numeric() {
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

    let resp = svc.assign("age_exp", "user_1", "", &attrs(&[("age", "25")])).unwrap();
    assert!(!resp.variant_id.is_empty(), "age 25 > 18 should match");

    let resp2 = svc.assign("age_exp", "user_2", "", &attrs(&[("age", "15")])).unwrap();
    assert!(resp2.variant_id.is_empty(), "age 15 <= 18 should not match");
}

// ── M1.5 Tests (session-level + layer-aware assignment) ──

#[test]
fn session_level_determinism() {
    let svc = make_service();
    let r1 = svc.assign("exp_dev_003", "user_1", "session_abc", &no_attrs()).unwrap();
    let r2 = svc.assign("exp_dev_003", "user_1", "session_abc", &no_attrs()).unwrap();
    assert_eq!(r1.variant_id, r2.variant_id, "same session_id must get same variant");
    assert!(r1.is_active);
    assert!(!r1.variant_id.is_empty());
}

#[test]
fn session_level_cross_session_variation() {
    let svc = make_service();
    // Collect variants across many sessions — should see both variants.
    let mut variants = std::collections::HashSet::new();
    for i in 0..200 {
        let session = format!("session_{i}");
        let resp = svc.assign("exp_dev_003", "user_1", &session, &no_attrs()).unwrap();
        variants.insert(resp.variant_id);
    }
    assert!(
        variants.len() >= 2,
        "expected at least 2 distinct variants across sessions, got {variants:?}"
    );
}

#[test]
fn session_level_missing_session_id() {
    let svc = make_service();
    let err = svc.assign("exp_dev_003", "user_1", "", &no_attrs()).unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(
        err.message().contains("session_id"),
        "error should mention session_id: {}",
        err.message()
    );
}

#[test]
fn session_level_same_user_different_sessions() {
    let svc = make_service();
    let r1 = svc.assign("exp_dev_003", "user_42", "sess_A", &no_attrs()).unwrap();
    let r2 = svc.assign("exp_dev_003", "user_42", "sess_B", &no_attrs()).unwrap();
    // Different sessions may get different buckets (hash depends on session_id, not user_id).
    // We can't guarantee different variants, but we verify both succeed.
    assert!(r1.is_active);
    assert!(r2.is_active);
    assert!(!r1.variant_id.is_empty());
    assert!(!r2.variant_id.is_empty());
}

#[test]
fn layer_orthogonality() {
    // User gets assignment in both default layer (AB) and session layer (SESSION_LEVEL).
    let svc = make_service();
    let ab_resp = svc.assign("exp_dev_001", "user_1", "", &no_attrs()).unwrap();
    let session_resp = svc.assign("exp_dev_003", "user_1", "session_1", &no_attrs()).unwrap();
    assert!(ab_resp.is_active);
    assert!(session_resp.is_active);
    assert!(!ab_resp.variant_id.is_empty(), "AB experiment should assign");
    assert!(!session_resp.variant_id.is_empty(), "session experiment should assign");
}

#[test]
fn layer_exclusive_allocation() {
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
        let a = svc.assign("exp_a", &user, "", &no_attrs()).unwrap();
        let b = svc.assign("exp_b", &user, "", &no_attrs()).unwrap();

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
        RankedList { item_ids: a_items.iter().map(|s| s.to_string()).collect() },
    );
    m.insert(
        "algo_b".to_string(),
        RankedList { item_ids: b_items.iter().map(|s| s.to_string()).collect() },
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

    assert!(!resp.merged_list.is_empty(), "merged list should not be empty");
    assert!(resp.merged_list.len() <= 10, "should not exceed max_list_size");
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
    let lists = make_algo_lists(
        &["x1", "x2", "x3", "x4"],
        &["y1", "y2", "y3", "y4"],
    );
    let r1 = svc.interleave("exp_dev_004", "user_42", &lists).unwrap();
    let r2 = svc.interleave("exp_dev_004", "user_42", &lists).unwrap();
    assert_eq!(r1.merged_list, r2.merged_list, "same inputs must produce same output");
    assert_eq!(r1.provenance, r2.provenance);
}

#[test]
fn interleaving_respects_max_list_size() {
    // exp_dev_004 has max_list_size=10. Provide 20 items total.
    let svc = make_service();
    let a: Vec<&str> = (0..10).map(|i| ["a0","a1","a2","a3","a4","a5","a6","a7","a8","a9"][i]).collect();
    let b: Vec<&str> = (0..10).map(|i| ["b0","b1","b2","b3","b4","b5","b6","b7","b8","b9"][i]).collect();
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
    let err = svc.interleave("nonexistent_exp", "user_1", &lists).unwrap_err();
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
    one.insert("algo_a".to_string(), RankedList { item_ids: vec!["x".to_string()] });
    let err = svc.interleave("exp_dev_004", "user_1", &one).unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    // 3 algorithms
    let mut three = HashMap::new();
    three.insert("algo_a".to_string(), RankedList { item_ids: vec!["x".to_string()] });
    three.insert("algo_b".to_string(), RankedList { item_ids: vec!["y".to_string()] });
    three.insert("algo_c".to_string(), RankedList { item_ids: vec!["z".to_string()] });
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
    assert_eq!(resp.merged_list.len(), 6, "all disjoint items should appear");
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
        let lists = make_algo_lists(
            &["i1", "i2", "i3", "i4"],
            &["i5", "i6", "i7", "i8"],
        );
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

// ── Bandit Delegation Tests (MAB / CONTEXTUAL_BANDIT) ──

#[test]
fn bandit_mab_basic_assignment() {
    let svc = make_service();
    let resp = svc.assign("exp_dev_005", "user_1", "", &no_attrs()).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty(), "MAB experiment should assign an arm");
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

#[test]
fn bandit_mab_deterministic() {
    let svc = make_service();
    let r1 = svc.assign("exp_dev_005", "user_42", "", &no_attrs()).unwrap();
    let r2 = svc.assign("exp_dev_005", "user_42", "", &no_attrs()).unwrap();
    assert_eq!(r1.variant_id, r2.variant_id, "same user must get same arm");
    assert!(
        (r1.assignment_probability - r2.assignment_probability).abs() < f64::EPSILON
    );
}

#[test]
fn bandit_mab_balance() {
    let svc = make_service();
    let mut counts: HashMap<String, u64> = HashMap::new();

    for i in 0..3000 {
        let user = format!("bandit_user_{i}");
        let resp = svc.assign("exp_dev_005", &user, "", &no_attrs()).unwrap();
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

#[test]
fn bandit_mab_payload_propagated() {
    let svc = make_service();
    let resp = svc.assign("exp_dev_005", "user_1", "", &no_attrs()).unwrap();
    // Payload should contain the arm's payload_json from bandit_config.
    assert!(
        resp.payload_json.contains("placement"),
        "payload should contain placement config: {}",
        resp.payload_json
    );
}

#[test]
fn bandit_missing_config_fails() {
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

    let err = svc.assign("bad_mab", "user_1", "", &no_attrs()).unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("bandit_config"));
}

#[test]
fn bandit_empty_arms_fails() {
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

    let err = svc.assign("empty_arms_mab", "user_1", "", &no_attrs()).unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("no arms"));
}

#[test]
fn bandit_contextual_type_also_delegates() {
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

    let resp = svc.assign("ctx_bandit", "user_1", "", &no_attrs()).unwrap();
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

#[test]
fn bandit_not_running_returns_inactive() {
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

    let resp = svc.assign("draft_mab", "user_1", "", &no_attrs()).unwrap();
    assert!(!resp.is_active, "DRAFT MAB should return is_active=false");
    assert!(resp.variant_id.is_empty());
}

#[test]
fn bandit_targeting_still_applies() {
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
    let resp = svc.assign("targeted_mab", "user_1", "", &attrs(&[("country", "US")])).unwrap();
    assert!(!resp.variant_id.is_empty(), "US user should get an arm");

    // User with FR country → targeting miss
    let resp2 = svc.assign("targeted_mab", "user_2", "", &attrs(&[("country", "FR")])).unwrap();
    assert!(resp2.variant_id.is_empty(), "FR user should not match targeting");
}
