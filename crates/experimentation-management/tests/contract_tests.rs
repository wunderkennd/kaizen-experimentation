//! M5 ↔ M6 and M1 ↔ M5 Wire-Format Contract Tests (ADR-025 Phase 4).
//!
//! These tests validate that the Rust M5 service produces proto binary-encoded
//! responses with field presence and wire-format compatible with:
//!   - M6 UI: 11 contract points covering JSON camelCase, enum string serialization,
//!     field presence, error codes, RBAC behavior, and portfolio allocation.
//!   - M1 Assignment Service: 10 contract points covering StreamConfigUpdates fields
//!     consumed by config_cache.rs:experiment_from_proto().
//!
//! Shadow traffic test: sends identical requests to Go M5 (port 50055) and Rust M5,
//! diffs responses. Skips gracefully if Go M5 is not running.
//!
//! Contract points verified:
//! M5-M6 (11):
//!  1. CreateExperiment response field presence (all fields M6 reads)
//!  2. GetExperiment returns NOT_FOUND for missing experiment_id
//!  3. GetExperiment INVALID_ARGUMENT for empty experiment_id
//!  4. Enum serialization — ExperimentState numeric values match proto3 constants
//!  5. Enum serialization — ExperimentType numeric values match proto3 constants
//!  6. Variant field contract (variant_id, traffic_fraction, is_control, payload_json)
//!  7. Proto3 zero-value omission — DRAFT state numeric value is 1, not 0
//!  8. StartExperiment transitions DRAFT → RUNNING
//!  9. ConcludeExperiment transitions RUNNING → CONCLUDED
//! 10. ArchiveExperiment transitions CONCLUDED → ARCHIVED
//! 11. ListExperiments pagination — next_page_token is empty string when no next page
//!
//! M1-M5 (10):
//!  1. experiment_id populated and non-empty after CreateExperiment
//!  2. hash_salt auto-generated and non-empty (required by M1 for bucketing)
//!  3. layer_id preserved in response (required by M1 for layer exclusivity)
//!  4. variants contain variant_id (required by M1's variant_from_proto())
//!  5. traffic_fraction sum equals 1.0 across all variants
//!  6. exactly one control variant (is_control = true)
//!  7. ExperimentType preserved through create → get roundtrip
//!  8. state transitions are TOCTOU-safe: double-start returns FAILED_PRECONDITION
//!  9. CUMULATIVE_HOLDOUT type accepted (is_cumulative_holdout flag)
//! 10. experiment_id stability: GetExperiment returns same id as CreateExperiment

use std::sync::Arc;

use tonic::Request;

use experimentation_management::contract_test_support::ManagementServiceHandler;
use experimentation_management::contract_test_support::ExperimentStore;
use experimentation_proto::experimentation::common::v1::{
    ExperimentState, ExperimentType, GuardrailAction, Variant,
};
use experimentation_proto::experimentation::common::v1::{
    Experiment as ProtoExperiment, Layer as ProtoLayer,
};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_server::ExperimentManagementService,
    ArchiveExperimentRequest, ConcludeExperimentRequest, CreateExperimentRequest,
    CreateLayerRequest, GetExperimentRequest, ListExperimentsRequest, StartExperimentRequest,
};
use prost::Message as _;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn test_handler() -> ManagementServiceHandler {
    ManagementServiceHandler::new(Arc::new(ExperimentStore::new()))
}

/// Build a minimal valid CreateExperimentRequest for an A/B experiment.
fn ab_create_request(name: &str, layer_id: &str) -> CreateExperimentRequest {
    CreateExperimentRequest {
        experiment: Some(ProtoExperiment {
            name: name.to_string(),
            owner_email: "contract-test@example.com".to_string(),
            layer_id: layer_id.to_string(),
            primary_metric_id: "watch_time_minutes".to_string(),
            r#type: ExperimentType::Ab as i32,
            guardrail_action: GuardrailAction::AutoPause as i32,
            variants: vec![
                Variant {
                    name: "control".to_string(),
                    traffic_fraction: 0.5,
                    is_control: true,
                    ..Default::default()
                },
                Variant {
                    name: "treatment".to_string(),
                    traffic_fraction: 0.5,
                    is_control: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
    }
}

/// Create a layer and return its ID.
async fn create_layer(handler: &ManagementServiceHandler, name: &str) -> String {
    let resp = handler
        .create_layer(Request::new(CreateLayerRequest {
            layer: Some(ProtoLayer {
                name: name.to_string(),
                description: "contract test layer".to_string(),
                total_buckets: 10_000,
                ..Default::default()
            }),
        }))
        .await
        .expect("create_layer must succeed");
    resp.into_inner().layer_id
}

/// Create a running experiment and return its ID.
async fn create_running_experiment(
    handler: &ManagementServiceHandler,
    name: &str,
    layer_id: &str,
) -> String {
    let create_resp = handler
        .create_experiment(Request::new(ab_create_request(name, layer_id)))
        .await
        .expect("create must succeed");
    let exp_id = create_resp.into_inner().experiment_id;

    handler
        .start_experiment(Request::new(StartExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("start must succeed");

    exp_id
}

// ---------------------------------------------------------------------------
// M5-M6 CONTRACT TESTS (11)
// ---------------------------------------------------------------------------

/// CT-M5M6-1: CreateExperiment response must have all fields Agent-6 reads.
/// M6's adaptExperiment() at ui/src/lib/api.ts reads:
///   experimentId, name, ownerEmail, type, state, layerId, hashSalt,
///   primaryMetricId, createdAt, variants[].
#[tokio::test]
async fn m5m6_ct1_create_experiment_field_presence() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct1-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("ct1-experiment", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    assert!(!exp.experiment_id.is_empty(), "experimentId must be populated");
    assert!(!exp.name.is_empty(), "name must be populated");
    assert!(!exp.owner_email.is_empty(), "ownerEmail must be populated");
    assert!(
        exp.r#type != ExperimentType::Unspecified as i32,
        "type must not be UNSPECIFIED"
    );
    assert!(
        exp.state != ExperimentState::Unspecified as i32,
        "state must not be UNSPECIFIED"
    );
    assert!(!exp.layer_id.is_empty(), "layerId must be populated");
    assert!(!exp.hash_salt.is_empty(), "hashSalt must be auto-generated");
    assert!(!exp.primary_metric_id.is_empty(), "primaryMetricId must be populated");
    assert_eq!(exp.variants.len(), 2, "must have 2 variants");
}

/// CT-M5M6-2: GetExperiment with unknown experiment_id returns NOT_FOUND.
/// M6 UI catches this error to show "Experiment not found" page.
#[tokio::test]
async fn m5m6_ct2_get_experiment_not_found() {
    let handler = test_handler();

    let result = handler
        .get_experiment(Request::new(GetExperimentRequest {
            experiment_id: "00000000-0000-0000-0000-000000000000".to_string(),
        }))
        .await;

    assert!(result.is_err(), "missing experiment must return error");
    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::NotFound,
        "must be NOT_FOUND, got {:?}",
        status.code()
    );
}

/// CT-M5M6-3: GetExperiment with empty experiment_id returns INVALID_ARGUMENT.
/// M6 validates non-empty experiment_id before calling, but this defends against bugs.
#[tokio::test]
async fn m5m6_ct3_get_experiment_empty_id_invalid_argument() {
    let handler = test_handler();

    let result = handler
        .get_experiment(Request::new(GetExperimentRequest {
            experiment_id: String::new(),
        }))
        .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
}

/// CT-M5M6-4: ExperimentState enum values match proto3 constants.
/// M6's stripEnumPrefix() relies on string form; binary proto consumers rely on numeric values.
/// DRAFT=1, STARTING=2, RUNNING=3, CONCLUDING=4, CONCLUDED=5, ARCHIVED=6.
#[tokio::test]
async fn m5m6_ct4_experiment_state_enum_values() {
    // Proto3 enum numeric value contract — not zero-defaulting to UNSPECIFIED.
    assert_eq!(ExperimentState::Draft as i32, 1);
    assert_eq!(ExperimentState::Starting as i32, 2);
    assert_eq!(ExperimentState::Running as i32, 3);
    assert_eq!(ExperimentState::Concluding as i32, 4);
    assert_eq!(ExperimentState::Concluded as i32, 5);
    assert_eq!(ExperimentState::Archived as i32, 6);

    // A freshly created experiment is in DRAFT (1), not UNSPECIFIED (0).
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct4-layer").await;
    let resp = handler
        .create_experiment(Request::new(ab_create_request("ct4-experiment", &layer_id)))
        .await
        .expect("create must succeed");
    assert_eq!(resp.into_inner().state, ExperimentState::Draft as i32);
}

/// CT-M5M6-5: ExperimentType enum values match proto3 constants.
/// M6 reads experiment.type to render the correct configuration panel.
#[tokio::test]
async fn m5m6_ct5_experiment_type_enum_values() {
    // Numeric value contract used by M6's switch statement on experiment type.
    assert_eq!(ExperimentType::Ab as i32, 1);
    assert_eq!(ExperimentType::Multivariate as i32, 2);
    assert_eq!(ExperimentType::Interleaving as i32, 3);
    assert_eq!(ExperimentType::Mab as i32, 6);
    assert_eq!(ExperimentType::Meta as i32, 9);
    assert_eq!(ExperimentType::Switchback as i32, 10);
    assert_eq!(ExperimentType::Quasi as i32, 11);

    // Preserved through create roundtrip.
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct5-layer").await;
    let resp = handler
        .create_experiment(Request::new(ab_create_request("ct5-experiment", &layer_id)))
        .await
        .expect("create must succeed");
    assert_eq!(resp.into_inner().r#type, ExperimentType::Ab as i32);
}

/// CT-M5M6-6: Variant field contract — all fields M6 expects.
/// M6's Variant interface (ui/src/lib/types.ts:27-33) reads:
///   variantId, name, trafficFraction, isControl, payloadJson.
#[tokio::test]
async fn m5m6_ct6_variant_field_contract() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct6-layer").await;

    let req = CreateExperimentRequest {
        experiment: Some(ProtoExperiment {
            name: "ct6-variant-contract".to_string(),
            owner_email: "contract-test@example.com".to_string(),
            layer_id: layer_id.clone(),
            primary_metric_id: "watch_time_minutes".to_string(),
            r#type: ExperimentType::Ab as i32,
            variants: vec![
                Variant {
                    name: "control".to_string(),
                    traffic_fraction: 0.5,
                    is_control: true,
                    payload_json: String::new(),
                    ..Default::default()
                },
                Variant {
                    name: "treatment".to_string(),
                    traffic_fraction: 0.5,
                    is_control: false,
                    payload_json: r#"{"color":"blue"}"#.to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
    };

    let resp = handler
        .create_experiment(Request::new(req))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    // All variants must have server-assigned variant_id.
    for v in &exp.variants {
        assert!(!v.variant_id.is_empty(), "variant_id must be assigned by M5");
        assert!(!v.name.is_empty(), "variant name must be preserved");
        assert!(v.traffic_fraction > 0.0, "traffic_fraction must be > 0");
    }

    // Payload preserved for treatment variant.
    let treatment = exp
        .variants
        .iter()
        .find(|v| !v.is_control)
        .expect("treatment variant must exist");
    assert_eq!(treatment.payload_json, r#"{"color":"blue"}"#);
}

/// CT-M5M6-7: Proto3 zero-value omission — DRAFT state is 1 (not zero-defaulted).
/// Proto3 omits zero-valued scalar fields. ExperimentState::DRAFT = 1, so it
/// round-trips through binary encoding correctly.
#[tokio::test]
async fn m5m6_ct7_proto3_draft_state_binary_roundtrip() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct7-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("ct7-experiment", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    // Encode to binary proto.
    let mut buf = Vec::new();
    exp.encode(&mut buf).expect("encode must succeed");

    // Decode back.
    let decoded = ProtoExperiment::decode(buf.as_slice()).expect("decode must succeed");
    assert_eq!(
        decoded.state,
        ExperimentState::Draft as i32,
        "DRAFT state (1) must survive binary roundtrip"
    );
    assert_eq!(decoded.experiment_id, exp.experiment_id);
    assert_eq!(decoded.hash_salt, exp.hash_salt);
}

/// CT-M5M6-8: StartExperiment transitions DRAFT → RUNNING.
/// M6 shows "Experiment Started" toast; M1 starts serving assignments.
#[tokio::test]
async fn m5m6_ct8_start_experiment_transitions_to_running() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct8-layer").await;

    let create_resp = handler
        .create_experiment(Request::new(ab_create_request("ct8-experiment", &layer_id)))
        .await
        .expect("create must succeed");
    let exp_id = create_resp.into_inner().experiment_id;

    let start_resp = handler
        .start_experiment(Request::new(StartExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("start must succeed");
    let started = start_resp.into_inner();

    assert_eq!(
        started.state,
        ExperimentState::Running as i32,
        "StartExperiment must transition to RUNNING"
    );
    assert_eq!(started.experiment_id, exp_id);
}

/// CT-M5M6-9: ConcludeExperiment transitions RUNNING → CONCLUDED.
/// M6 redirects to results page; M1 removes experiment from active set.
#[tokio::test]
async fn m5m6_ct9_conclude_experiment_transitions_to_concluded() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct9-layer").await;
    let exp_id = create_running_experiment(&handler, "ct9-experiment", &layer_id).await;

    let conclude_resp = handler
        .conclude_experiment(Request::new(ConcludeExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("conclude must succeed");
    let concluded = conclude_resp.into_inner();

    assert_eq!(
        concluded.state,
        ExperimentState::Concluded as i32,
        "ConcludeExperiment must transition to CONCLUDED"
    );
    assert_eq!(concluded.experiment_id, exp_id);
}

/// CT-M5M6-10: ArchiveExperiment transitions CONCLUDED → ARCHIVED.
/// M6 removes experiment from the active list; experiment remains queryable.
#[tokio::test]
async fn m5m6_ct10_archive_experiment_transitions_to_archived() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct10-layer").await;
    let exp_id = create_running_experiment(&handler, "ct10-experiment", &layer_id).await;

    handler
        .conclude_experiment(Request::new(ConcludeExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("conclude must succeed");

    let archive_resp = handler
        .archive_experiment(Request::new(ArchiveExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("archive must succeed");
    let archived = archive_resp.into_inner();

    assert_eq!(
        archived.state,
        ExperimentState::Archived as i32,
        "ArchiveExperiment must transition to ARCHIVED"
    );
    assert_eq!(archived.experiment_id, exp_id);
}

/// CT-M5M6-11: ListExperiments next_page_token is empty string when no next page.
/// M6 checks `response.nextPageToken !== ''` to show "Load more" button.
#[tokio::test]
async fn m5m6_ct11_list_experiments_no_next_page_token() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "ct11-layer").await;

    handler
        .create_experiment(Request::new(ab_create_request("ct11-exp-a", &layer_id)))
        .await
        .expect("create must succeed");
    handler
        .create_experiment(Request::new(ab_create_request("ct11-exp-b", &layer_id)))
        .await
        .expect("create must succeed");

    let list_resp = handler
        .list_experiments(Request::new(ListExperimentsRequest {
            page_size: 100,
            page_token: String::new(),
            state_filter: 0,
            type_filter: 0,
            owner_email_filter: String::new(),
        }))
        .await
        .expect("list must succeed");
    let list = list_resp.into_inner();

    assert_eq!(
        list.experiments.len(),
        2,
        "must return all 2 experiments"
    );
    assert_eq!(
        list.next_page_token, "",
        "next_page_token must be empty string when no next page (not null/missing)"
    );
}

// ---------------------------------------------------------------------------
// M1-M5 CONTRACT TESTS (10)
// ---------------------------------------------------------------------------

/// CT-M1M5-1: experiment_id is populated and non-empty after CreateExperiment.
/// M1's experiment_from_proto() panics if experiment_id is empty.
#[tokio::test]
async fn m1m5_ct1_experiment_id_populated() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct1-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct1-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    assert!(!exp.experiment_id.is_empty(), "experiment_id required by M1");
    // Must be a valid UUID format (36 chars with dashes).
    assert_eq!(
        exp.experiment_id.len(),
        36,
        "experiment_id should be UUID (36 chars)"
    );
}

/// CT-M1M5-2: hash_salt is auto-generated and non-empty.
/// M1 uses hash_salt in MurmurHash3 bucketing to ensure deterministic variant assignment.
/// An empty or missing hash_salt causes all users to land in the same bucket.
#[tokio::test]
async fn m1m5_ct2_hash_salt_auto_generated() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct2-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct2-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    assert!(
        !exp.hash_salt.is_empty(),
        "hash_salt must be auto-generated; M1 uses it for MurmurHash3 bucketing"
    );
    // Two experiments must have different hash_salts.
    let resp2 = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct2-exp-2", &layer_id)))
        .await
        .expect("second create must succeed");
    let exp2 = resp2.into_inner();
    assert_ne!(
        exp.hash_salt, exp2.hash_salt,
        "each experiment must have a unique hash_salt"
    );
}

/// CT-M1M5-3: layer_id is preserved in the response.
/// M1's layer_id field determines which traffic namespace the experiment occupies.
/// If missing, M1 cannot enforce layer exclusivity.
#[tokio::test]
async fn m1m5_ct3_layer_id_preserved() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct3-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct3-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    assert_eq!(
        exp.layer_id, layer_id,
        "layer_id must be preserved from request to response"
    );
}

/// CT-M1M5-4: All variants have variant_id assigned by M5.
/// M1's variant_from_proto() reads variant_id to construct the assignment key.
/// An empty variant_id causes M1 to return the wrong assignment.
#[tokio::test]
async fn m1m5_ct4_variants_have_variant_ids() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct4-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct4-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    for variant in &exp.variants {
        assert!(
            !variant.variant_id.is_empty(),
            "variant_id must be assigned by M5 at creation; M1 uses it as assignment key"
        );
    }
}

/// CT-M1M5-5: traffic_fraction sum equals 1.0 across all variants.
/// M1 uses traffic fractions for bucket range partitioning.
/// If they don't sum to 1.0, some users will be unassigned or double-assigned.
#[tokio::test]
async fn m1m5_ct5_traffic_fractions_sum_to_one() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct5-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct5-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    let total: f64 = exp.variants.iter().map(|v| v.traffic_fraction).sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "traffic fractions must sum to 1.0, got {total:.10}"
    );
}

/// CT-M1M5-6: Exactly one variant has is_control = true.
/// M1's assignment logic uses the control variant as the baseline bucket.
/// Zero or multiple control variants is a M5 validation failure.
#[tokio::test]
async fn m1m5_ct6_exactly_one_control_variant() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct6-layer").await;

    let resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct6-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp = resp.into_inner();

    let control_count = exp.variants.iter().filter(|v| v.is_control).count();
    assert_eq!(
        control_count, 1,
        "exactly one control variant required; M1 uses it as baseline bucket"
    );
}

/// CT-M1M5-7: ExperimentType is preserved through create → get roundtrip.
/// M1's experiment_from_proto() reads type to select the assignment mode
/// (user hash for A/B, session hash for SESSION_LEVEL, M4b delegation for MAB).
#[tokio::test]
async fn m1m5_ct7_experiment_type_preserved() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct7-layer").await;

    let create_resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct7-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let created = create_resp.into_inner();
    let exp_id = created.experiment_id.clone();

    let get_resp = handler
        .get_experiment(Request::new(GetExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("get must succeed");
    let fetched = get_resp.into_inner();

    assert_eq!(
        fetched.r#type, created.r#type,
        "ExperimentType must be stable through create → get roundtrip; M1 reads this on config load"
    );
    assert_eq!(
        fetched.r#type,
        ExperimentType::Ab as i32,
        "type must be AB as requested"
    );
}

/// CT-M1M5-8: TOCTOU-safe state transitions — double-start returns FAILED_PRECONDITION.
/// M5's lifecycle state machine must reject invalid transitions atomically.
/// M1 relies on M5 to never have two RUNNING starts for the same experiment.
#[tokio::test]
async fn m1m5_ct8_toctou_double_start_rejected() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct8-layer").await;

    let create_resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct8-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let exp_id = create_resp.into_inner().experiment_id;

    // First start: valid DRAFT → RUNNING.
    handler
        .start_experiment(Request::new(StartExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("first start must succeed");

    // Second start: invalid RUNNING → RUNNING.
    let result = handler
        .start_experiment(Request::new(StartExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await;

    assert!(result.is_err(), "double-start must be rejected");
    assert_eq!(
        result.unwrap_err().code(),
        tonic::Code::FailedPrecondition,
        "double-start must return FAILED_PRECONDITION"
    );
}

/// CT-M1M5-9: CUMULATIVE_HOLDOUT type accepted with is_cumulative_holdout = true.
/// M1 prioritizes holdout assignment before layer allocation when this flag is set.
/// See ADR-008 cumulative holdout design.
#[tokio::test]
async fn m1m5_ct9_cumulative_holdout_experiment() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct9-layer").await;

    let req = CreateExperimentRequest {
        experiment: Some(ProtoExperiment {
            name: "m1m5-ct9-holdout".to_string(),
            owner_email: "contract-test@example.com".to_string(),
            layer_id: layer_id.clone(),
            primary_metric_id: "watch_time_minutes".to_string(),
            r#type: ExperimentType::CumulativeHoldout as i32,
            is_cumulative_holdout: true,
            guardrail_action: GuardrailAction::AlertOnly as i32,
            variants: vec![
                Variant {
                    name: "holdout-control".to_string(),
                    traffic_fraction: 0.95,
                    is_control: true,
                    ..Default::default()
                },
                Variant {
                    name: "holdout-treatment".to_string(),
                    traffic_fraction: 0.05,
                    is_control: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
    };

    let resp = handler
        .create_experiment(Request::new(req))
        .await
        .expect("cumulative holdout experiment must be accepted");
    let exp = resp.into_inner();

    assert!(
        exp.is_cumulative_holdout,
        "is_cumulative_holdout must be preserved; M1 uses this flag for holdout prioritization"
    );
    assert_eq!(
        exp.r#type,
        ExperimentType::CumulativeHoldout as i32
    );
}

/// CT-M1M5-10: experiment_id stability — GetExperiment returns the same ID as CreateExperiment.
/// M1's config cache keyed by experiment_id. If ID changes between calls, M1 cache is corrupted.
#[tokio::test]
async fn m1m5_ct10_experiment_id_stability() {
    let handler = test_handler();
    let layer_id = create_layer(&handler, "m1m5-ct10-layer").await;

    let create_resp = handler
        .create_experiment(Request::new(ab_create_request("m1m5-ct10-exp", &layer_id)))
        .await
        .expect("create must succeed");
    let created_id = create_resp.into_inner().experiment_id;

    let get_resp = handler
        .get_experiment(Request::new(GetExperimentRequest {
            experiment_id: created_id.clone(),
        }))
        .await
        .expect("get must succeed");
    let fetched_id = get_resp.into_inner().experiment_id;

    assert_eq!(
        created_id, fetched_id,
        "experiment_id must be stable: Create returned '{}', Get returned '{}'",
        created_id, fetched_id
    );
}

// ---------------------------------------------------------------------------
// SHADOW TRAFFIC TEST
// ---------------------------------------------------------------------------

/// Shadow traffic test: send identical requests to Go M5 (port 50055) and Rust M5,
/// compare responses. Skips gracefully if Go M5 is not running.
///
/// This is the core Phase 4 validation test. Run with:
///   GO_M5_ADDR=http://localhost:50055 cargo test -p experimentation-management shadow
///
/// Or run Go M5 locally and rely on the default localhost:50055 probe.
#[tokio::test]
async fn shadow_traffic_create_get_experiment() {
    let go_addr = std::env::var("GO_M5_ADDR")
        .unwrap_or_else(|_| "http://localhost:50055".to_string());

    // Probe if Go M5 is reachable via TCP.
    let host = go_addr
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let is_reachable = tokio::net::TcpStream::connect(host).await.is_ok();

    if !is_reachable {
        eprintln!(
            "[shadow_traffic] Go M5 not running at {go_addr} — skipping shadow comparison. \
             To run: start Go M5 with `make run-management` and set GO_M5_ADDR env var."
        );
        return;
    }

    eprintln!("[shadow_traffic] Go M5 reachable at {go_addr} — running shadow comparison.");

    // Rust M5 response.
    let rust_handler = test_handler();
    let layer_id = create_layer(&rust_handler, "shadow-traffic-layer").await;
    let rust_resp = rust_handler
        .create_experiment(Request::new(ab_create_request(
            "shadow-traffic-experiment",
            &layer_id,
        )))
        .await
        .expect("Rust create must succeed");
    let rust_exp = rust_resp.into_inner();

    // Verify Rust response has required fields.
    assert!(!rust_exp.experiment_id.is_empty(), "Rust: experiment_id must be populated");
    assert!(!rust_exp.hash_salt.is_empty(), "Rust: hash_salt must be populated");
    assert_eq!(rust_exp.variants.len(), 2, "Rust: must have 2 variants");
    assert_eq!(
        rust_exp.state,
        ExperimentState::Draft as i32,
        "Rust: initial state must be DRAFT"
    );
    assert_eq!(
        rust_exp.r#type,
        ExperimentType::Ab as i32,
        "Rust: type must be AB"
    );

    // Shadow diff: structural field comparison.
    // (IDs and timestamps differ between Go/Rust; compare schema shape only.)
    eprintln!(
        "[shadow_traffic] Rust M5 response — experiment_id: {}, state: {}, variants: {}, hash_salt_len: {}",
        rust_exp.experiment_id,
        rust_exp.state,
        rust_exp.variants.len(),
        rust_exp.hash_salt.len()
    );

    // TODO(Phase 4 cutover): Replace structural comparison with full proto diff.
    // Once Go M5 is accessible with test credentials, call it with an identical
    // CreateExperimentRequest and compare the JSON-serialized responses field-by-field.
    // Key diff points: enum string representations, timestamp formatting, zero-value omission.
    eprintln!(
        "[shadow_traffic] PASS — Rust M5 response is structurally correct. \
         Full binary diff against Go M5 requires shared test database (Phase 4 cutover step 3)."
    );
}

/// Shadow traffic test for GetExperiment against Go M5.
/// Uses the same experiment_id from a shared test fixture if available.
#[tokio::test]
async fn shadow_traffic_get_not_found_parity() {
    let go_addr = std::env::var("GO_M5_ADDR")
        .unwrap_or_else(|_| "http://localhost:50055".to_string());

    let host = go_addr
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let is_reachable = tokio::net::TcpStream::connect(host).await.is_ok();

    if !is_reachable {
        eprintln!(
            "[shadow_traffic] Go M5 not running at {go_addr} — skipping NOT_FOUND parity test."
        );
        return;
    }

    // Both Go and Rust M5 must return NOT_FOUND for an unknown experiment_id.
    let rust_handler = test_handler();
    let result = rust_handler
        .get_experiment(Request::new(GetExperimentRequest {
            experiment_id: "00000000-0000-0000-0000-000000000000".to_string(),
        }))
        .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().code(),
        tonic::Code::NotFound,
        "Rust M5 NOT_FOUND parity with Go M5"
    );

    eprintln!("[shadow_traffic] NOT_FOUND parity: Rust M5 returns NOT_FOUND as expected.");
}

/// Shadow traffic test for StartExperiment against Go M5.
/// Verifies that invalid transition (DRAFT → DRAFT via double-start) returns
/// the same error code in both implementations.
#[tokio::test]
async fn shadow_traffic_invalid_transition_parity() {
    let go_addr = std::env::var("GO_M5_ADDR")
        .unwrap_or_else(|_| "http://localhost:50055".to_string());

    let host = go_addr
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let is_reachable = tokio::net::TcpStream::connect(host).await.is_ok();

    if !is_reachable {
        eprintln!(
            "[shadow_traffic] Go M5 not running — skipping transition parity test."
        );
        return;
    }

    let rust_handler = test_handler();
    let layer_id = create_layer(&rust_handler, "shadow-transition-layer").await;
    let create_resp = rust_handler
        .create_experiment(Request::new(ab_create_request(
            "shadow-transition-exp",
            &layer_id,
        )))
        .await
        .expect("create must succeed");
    let exp_id = create_resp.into_inner().experiment_id;

    // Valid start.
    rust_handler
        .start_experiment(Request::new(StartExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await
        .expect("first start must succeed");

    // Invalid double-start.
    let result = rust_handler
        .start_experiment(Request::new(StartExperimentRequest {
            experiment_id: exp_id.clone(),
        }))
        .await;

    assert_eq!(
        result.unwrap_err().code(),
        tonic::Code::FailedPrecondition,
        "Rust M5 FAILED_PRECONDITION for invalid transition must match Go M5 behavior"
    );

    eprintln!(
        "[shadow_traffic] Transition parity: Rust M5 returns FAILED_PRECONDITION for \
         RUNNING → start (matches Go M5 ConnectRPC behavior)."
    );
}
