use std::collections::HashMap;
use std::sync::Arc;

use experimentation_assignment::config::Config;
use experimentation_assignment::service::AssignmentServiceImpl;

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
    let r1 = svc.assign("exp_dev_001", "user_42", &no_attrs()).unwrap();
    let r2 = svc.assign("exp_dev_001", "user_42", &no_attrs()).unwrap();
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
        let resp = svc.assign("exp_dev_001", &user_id, &no_attrs()).unwrap();
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
    let err = svc.assign("nonexistent_exp", "user_1", &no_attrs()).unwrap_err();
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
        let resp = svc.assign("narrow_exp", &format!("user_{i}"), &no_attrs()).unwrap();
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

    let resp = svc.assign("draft_exp", "user_1", &no_attrs()).unwrap();
    assert!(!resp.is_active, "DRAFT experiment must return is_active=false");
    assert!(resp.variant_id.is_empty(), "DRAFT experiment must not assign a variant");
}

// ── M1.4 Tests (targeting rules) ──

#[test]
fn targeting_country_in_match() {
    let svc = make_service();
    // exp_dev_002 targets country IN [US, UK] AND tier IN [premium, platinum]
    let resp = svc.assign("exp_dev_002", "user_1", &attrs(&[("country", "US"), ("tier", "premium")])).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty(), "targeted user should get a variant");
}

#[test]
fn targeting_empty_rule_matches_all() {
    let svc = make_service();
    // exp_dev_001 has no targeting rule — all users match
    let resp = svc.assign("exp_dev_001", "user_1", &no_attrs()).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty());
}

#[test]
fn targeting_missing_attribute_no_match() {
    let svc = make_service();
    // exp_dev_002 requires country + tier, but we provide neither
    let resp = svc.assign("exp_dev_002", "user_1", &no_attrs()).unwrap();
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

    let resp = svc.assign("no_target_exp", "user_1", &no_attrs()).unwrap();
    assert!(resp.is_active);
    assert!(!resp.variant_id.is_empty(), "no targeting rule → all users match");
}

#[test]
fn targeting_compound_and_across_groups() {
    let svc = make_service();
    // Correct country but wrong tier → AND fails
    let resp = svc.assign("exp_dev_002", "user_1", &attrs(&[("country", "US"), ("tier", "free")])).unwrap();
    assert!(resp.variant_id.is_empty(), "wrong tier should not match");

    // Correct tier but wrong country → AND fails
    let resp2 = svc.assign("exp_dev_002", "user_2", &attrs(&[("country", "FR"), ("tier", "premium")])).unwrap();
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

    let resp = svc.assign("age_exp", "user_1", &attrs(&[("age", "25")])).unwrap();
    assert!(!resp.variant_id.is_empty(), "age 25 > 18 should match");

    let resp2 = svc.assign("age_exp", "user_2", &attrs(&[("age", "15")])).unwrap();
    assert!(resp2.variant_id.is_empty(), "age 15 <= 18 should not match");
}
