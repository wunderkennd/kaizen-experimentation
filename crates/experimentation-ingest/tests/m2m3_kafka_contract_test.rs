//! M2 → M3 ModelRetrainingEvent Kafka Contract Tests (ADR-021)
//!
//! These tests verify the proto wire-format contract for `ModelRetrainingEvent`
//! between M2 (Event Ingestion, producer) and M3 (Metrics, consumer).
//!
//! M2 validates events via `validate_model_retraining_event`, then serializes
//! them to proto bytes for the `model_retraining_events` Kafka topic.
//! M3 deserializes those bytes and joins them with exposure data to compute
//! training data contamination fractions for feedback loop analysis.
//!
//! Since Kafka transport is tested separately, these tests use prost encode/decode
//! directly to verify the wire format without requiring a running Kafka cluster.
//!
//! Contract points verified:
//! 1. All required fields survive proto roundtrip: event_id, model_id, timestamps
//! 2. active_experiment_ids list is preserved (including empty and multi-element)
//! 3. treatment_contamination_fraction round-trips without precision loss
//! 4. training_data_start and training_data_end timestamps are encoded correctly
//! 5. retrained_at timestamp is optional and preserved when set
//! 6. M2's validation layer rejects missing required fields (consumer expectations)
//! 7. M2's validation rejects training_data_end <= training_data_start (window sanity)
//! 8. treatment_contamination_fraction=0.0 is a valid proto zero-value (not dropped)
//! 9. Large active_experiment_ids lists are preserved without truncation
//! 10. Proto field ordering contract: field numbers are stable across encode/decode

use experimentation_ingest::validation::validate_model_retraining_event;
use experimentation_proto::common::ModelRetrainingEvent;
use prost::Message;

// ---------------------------------------------------------------------------
// Helper builders
// ---------------------------------------------------------------------------

fn past_timestamp(offset_hours: i64) -> Option<prost_types::Timestamp> {
    let epoch_secs = chrono::Utc::now().timestamp() - offset_hours * 3600;
    Some(prost_types::Timestamp {
        seconds: epoch_secs,
        nanos: 0,
    })
}

/// Build a valid ModelRetrainingEvent that M2 would accept.
fn valid_retraining_event() -> ModelRetrainingEvent {
    ModelRetrainingEvent {
        event_id: "mre-contract-001".into(),
        model_id: "rec-model-v42".into(),
        training_data_start: past_timestamp(72),  // 3 days ago
        training_data_end: past_timestamp(24),    // 1 day ago
        retrained_at: past_timestamp(1),           // 1 hour ago (recent, use past_timestamp with small value)
        active_experiment_ids: vec![
            "exp-001".into(),
            "exp-002".into(),
            "exp-003".into(),
        ],
        treatment_contamination_fraction: 0.23,
    }
}

/// Encode a ModelRetrainingEvent to bytes and decode it back (simulates Kafka roundtrip).
fn kafka_roundtrip(event: &ModelRetrainingEvent) -> ModelRetrainingEvent {
    let mut buf = Vec::new();
    event.encode(&mut buf).expect("proto encode should not fail");
    ModelRetrainingEvent::decode(buf.as_slice()).expect("proto decode should not fail")
}

// ---------------------------------------------------------------------------
// Contract Tests
// ---------------------------------------------------------------------------

/// 1. Required fields survive proto roundtrip.
#[test]
fn contract_required_fields_roundtrip() {
    let original = valid_retraining_event();
    validate_model_retraining_event(&original).expect("valid event should pass M2 validation");

    let decoded = kafka_roundtrip(&original);

    assert_eq!(
        decoded.event_id, original.event_id,
        "event_id must survive proto roundtrip"
    );
    assert_eq!(
        decoded.model_id, original.model_id,
        "model_id must survive proto roundtrip"
    );
}

/// 2. training_data_start and training_data_end timestamps are encoded correctly.
#[test]
fn contract_training_window_timestamps_roundtrip() {
    let original = valid_retraining_event();
    let decoded = kafka_roundtrip(&original);

    let orig_start = original.training_data_start.as_ref().unwrap();
    let dec_start = decoded
        .training_data_start
        .as_ref()
        .expect("training_data_start must be present after decode");

    assert_eq!(
        dec_start.seconds, orig_start.seconds,
        "training_data_start.seconds must survive roundtrip"
    );
    assert_eq!(
        dec_start.nanos, orig_start.nanos,
        "training_data_start.nanos must survive roundtrip"
    );

    let orig_end = original.training_data_end.as_ref().unwrap();
    let dec_end = decoded
        .training_data_end
        .as_ref()
        .expect("training_data_end must be present after decode");

    assert_eq!(
        dec_end.seconds, orig_end.seconds,
        "training_data_end.seconds must survive roundtrip"
    );

    // M3 contract: end must be after start in decoded message.
    assert!(
        dec_end.seconds > dec_start.seconds,
        "training_data_end ({}) must be after training_data_start ({}) after decode",
        dec_end.seconds,
        dec_start.seconds
    );
}

/// 3. retrained_at timestamp is optional; preserved when set.
#[test]
fn contract_retrained_at_optional_preserved() {
    // With retrained_at set.
    let original = valid_retraining_event();
    let decoded = kafka_roundtrip(&original);

    let orig_ts = original.retrained_at.as_ref().unwrap();
    let dec_ts = decoded
        .retrained_at
        .as_ref()
        .expect("retrained_at should be present after decode");
    assert_eq!(dec_ts.seconds, orig_ts.seconds, "retrained_at.seconds must survive roundtrip");

    // Without retrained_at (proto3 zero-value → omitted).
    let mut no_ts = valid_retraining_event();
    no_ts.retrained_at = None;
    let decoded_no_ts = kafka_roundtrip(&no_ts);
    assert!(
        decoded_no_ts.retrained_at.is_none(),
        "retrained_at=None should remain None after roundtrip (proto3 optional semantics)"
    );
}

/// 4. active_experiment_ids list is preserved exactly (including order).
#[test]
fn contract_active_experiment_ids_roundtrip() {
    let original = valid_retraining_event();
    let decoded = kafka_roundtrip(&original);

    assert_eq!(
        decoded.active_experiment_ids, original.active_experiment_ids,
        "active_experiment_ids must be preserved exactly after proto roundtrip"
    );
    assert_eq!(
        decoded.active_experiment_ids.len(),
        3,
        "all 3 active_experiment_ids must survive encode/decode"
    );
}

/// 5. Empty active_experiment_ids is valid (proto3 zero-value).
#[test]
fn contract_active_experiment_ids_empty_valid() {
    let mut event = valid_retraining_event();
    event.active_experiment_ids = vec![];

    validate_model_retraining_event(&event).expect("empty active_experiment_ids is valid for M2");

    let decoded = kafka_roundtrip(&event);
    assert!(
        decoded.active_experiment_ids.is_empty(),
        "empty active_experiment_ids must survive roundtrip as empty (not None)"
    );
}

/// 6. Large active_experiment_ids list is preserved without truncation.
#[test]
fn contract_large_active_experiment_ids_no_truncation() {
    let mut event = valid_retraining_event();
    event.active_experiment_ids = (0..500).map(|i| format!("exp-{i:04}")).collect();

    let decoded = kafka_roundtrip(&event);

    assert_eq!(
        decoded.active_experiment_ids.len(),
        500,
        "500 active_experiment_ids must survive roundtrip without truncation"
    );
    // Spot-check a few entries.
    assert_eq!(decoded.active_experiment_ids[0], "exp-0000");
    assert_eq!(decoded.active_experiment_ids[499], "exp-0499");
}

/// 7. treatment_contamination_fraction survives roundtrip without precision loss.
#[test]
fn contract_contamination_fraction_roundtrip() {
    let fractions = [0.0, 0.001, 0.123456789, 0.5, 0.999, 1.0];

    for &fraction in &fractions {
        let mut event = valid_retraining_event();
        event.treatment_contamination_fraction = fraction;

        let decoded = kafka_roundtrip(&event);

        // f64 proto encoding is exact (IEEE 754 double).
        assert_eq!(
            decoded.treatment_contamination_fraction, fraction,
            "treatment_contamination_fraction {fraction} must survive roundtrip exactly"
        );
    }
}

/// 8. treatment_contamination_fraction=0.0 is not dropped (proto3 zero-value).
///
/// M3 uses this field for contamination analysis. If M2 sets it to 0.0
/// (no contamination) and proto3 drops it, M3 would also see 0.0 (the default),
/// so the wire format is correct — but we validate the behavior explicitly.
#[test]
fn contract_zero_contamination_fraction_preserved() {
    let mut event = valid_retraining_event();
    event.treatment_contamination_fraction = 0.0;

    let decoded = kafka_roundtrip(&event);

    // Proto3: 0.0 is the default for double fields; the field is not written
    // to the wire, and the decoder will return 0.0 (the default). This is
    // correct behavior — M3 sees 0.0 as expected.
    assert_eq!(
        decoded.treatment_contamination_fraction,
        0.0,
        "zero contamination fraction must decode to 0.0 (proto3 default behavior)"
    );
}

/// 9. M2 validation rejects missing event_id (consumer expectation).
#[test]
fn contract_m2_rejects_missing_event_id() {
    let mut event = valid_retraining_event();
    event.event_id = String::new();

    let err = validate_model_retraining_event(&event).unwrap_err();
    assert!(
        err.to_string().contains("event_id is required"),
        "M2 must reject missing event_id: {err}"
    );
}

/// 10. M2 validation rejects missing model_id.
#[test]
fn contract_m2_rejects_missing_model_id() {
    let mut event = valid_retraining_event();
    event.model_id = String::new();

    let err = validate_model_retraining_event(&event).unwrap_err();
    assert!(
        err.to_string().contains("model_id is required"),
        "M2 must reject missing model_id: {err}"
    );
}

/// 11. M2 validation rejects missing training_data_start.
#[test]
fn contract_m2_rejects_missing_training_data_start() {
    let mut event = valid_retraining_event();
    event.training_data_start = None;

    let err = validate_model_retraining_event(&event).unwrap_err();
    assert!(
        err.to_string().contains("training_data_start is required"),
        "M2 must reject missing training_data_start: {err}"
    );
}

/// 12. M2 validation rejects missing training_data_end.
#[test]
fn contract_m2_rejects_missing_training_data_end() {
    let mut event = valid_retraining_event();
    event.training_data_end = None;

    let err = validate_model_retraining_event(&event).unwrap_err();
    assert!(
        err.to_string().contains("training_data_end is required"),
        "M2 must reject missing training_data_end: {err}"
    );
}

/// 13. M2 validation rejects training_data_end <= training_data_start.
///
/// M3 uses the training window for temporal joins. An invalid window (end before start)
/// would produce empty joins or incorrect contamination fractions.
#[test]
fn contract_m2_rejects_inverted_training_window() {
    let mut event = valid_retraining_event();
    // Swap: end is older than start.
    event.training_data_start = past_timestamp(24);
    event.training_data_end = past_timestamp(72); // older than start

    let err = validate_model_retraining_event(&event).unwrap_err();
    assert!(
        err.to_string().contains("training_data_end must be after"),
        "M2 must reject inverted training window: {err}"
    );
}

/// 14. Historical training windows (> 24h ago) are valid for M2.
///
/// M3 frequently joins against historical exposure data. Training windows
/// can reference ranges weeks or months in the past — they are NOT subject
/// to the ±24h validation applied to ExposureEvent timestamps.
#[test]
fn contract_historical_training_window_accepted_by_m2() {
    let event = ModelRetrainingEvent {
        event_id: "mre-historical".into(),
        model_id: "rec-model-v1".into(),
        training_data_start: past_timestamp(30 * 24),  // 30 days ago
        training_data_end: past_timestamp(7 * 24),     // 7 days ago
        retrained_at: past_timestamp(1),
        active_experiment_ids: vec!["exp-historical".into()],
        treatment_contamination_fraction: 0.15,
    };

    validate_model_retraining_event(&event)
        .expect("historical training window must be valid for M2 (no ±24h restriction on retraining timestamps)");

    let decoded = kafka_roundtrip(&event);
    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(
        decoded.treatment_contamination_fraction,
        event.treatment_contamination_fraction
    );
}

/// 15. Wire size is reasonable: encoding a typical event produces < 1KB.
///
/// Kafka has a default max message size of 1MB; this sanity-checks that a
/// typical ModelRetrainingEvent with ~10 experiment IDs doesn't grow unexpectedly.
#[test]
fn contract_wire_size_typical_event() {
    let mut event = valid_retraining_event();
    event.active_experiment_ids = (0..10).map(|i| format!("exp-{i:03}")).collect();
    event.treatment_contamination_fraction = 0.12;

    let mut buf = Vec::new();
    event.encode(&mut buf).unwrap();

    assert!(
        buf.len() < 1024,
        "typical ModelRetrainingEvent should serialize to < 1KB, got {} bytes",
        buf.len()
    );
}

/// 16. Multiple encode/decode cycles produce identical bytes (idempotent serialization).
#[test]
fn contract_serialization_idempotent() {
    let original = valid_retraining_event();

    let mut buf1 = Vec::new();
    original.encode(&mut buf1).unwrap();

    let decoded = ModelRetrainingEvent::decode(buf1.as_slice()).unwrap();

    let mut buf2 = Vec::new();
    decoded.encode(&mut buf2).unwrap();

    assert_eq!(
        buf1, buf2,
        "two encode cycles of the same event must produce identical bytes"
    );
}
