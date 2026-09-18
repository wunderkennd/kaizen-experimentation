//! External producer → M2 ExposureEvent wire contract tests.
//!
//! External services (first consumer: the kaizen-accelerator gateway) emit
//! exposures by calling M2 `IngestExposure`/`IngestExposureBatch` with an
//! `experimentation.common.v1.ExposureEvent`. Those producers are NOT built
//! from this repo's prost bindings — they use their own protobuf stacks
//! (protoc-gen-go, vendored snapshots) — so this suite pins the wire format
//! from the producer's side: every message is hand-encoded from raw field
//! numbers and wire types, then pushed through M2's decode + validation +
//! dedup path exactly as ingest would.
//!
//! The canonical emission rules being pinned (documented on ExposureEvent in
//! proto/experimentation/common/v1/event.proto):
//! 1. Required fields: event_id(1), experiment_id(2), user_id(3),
//!    variant_id(4), timestamp(5) — ingest rejects an event missing any.
//! 2. timestamp must be within ±24h of ingest time, nanos >= 0.
//! 3. event_id is the global dedup identity (Bloom filter).
//! 4. assignment_probability(8) must be finite when set (NaN fails fast).
//! 5. Unknown fields from newer producers are tolerated (proto3 forward
//!    compatibility) — a producer one schema revision ahead still ingests.
//! 6. A minimal producer encoding (fields in ascending order, no map entries)
//!    is byte-identical to prost's canonical encoding of the same event.

use experimentation_ingest::dedup::EventDedup;
use experimentation_ingest::validation::validate_exposure;
use experimentation_proto::common::ExposureEvent;
use prost::Message;

// ---------------------------------------------------------------------------
// Minimal hand-rolled proto writer (the "external producer")
// ---------------------------------------------------------------------------

const WIRE_VARINT: u8 = 0;
const WIRE_I64: u8 = 1;
const WIRE_LEN: u8 = 2;

fn put_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

fn put_tag(buf: &mut Vec<u8>, field: u32, wire_type: u8) {
    put_varint(buf, ((field as u64) << 3) | wire_type as u64);
}

fn put_string(buf: &mut Vec<u8>, field: u32, value: &str) {
    put_tag(buf, field, WIRE_LEN);
    put_varint(buf, value.len() as u64);
    buf.extend_from_slice(value.as_bytes());
}

fn put_double(buf: &mut Vec<u8>, field: u32, value: f64) {
    put_tag(buf, field, WIRE_I64);
    buf.extend_from_slice(&value.to_bits().to_le_bytes());
}

/// google.protobuf.Timestamp { int64 seconds = 1; int32 nanos = 2; }
fn put_timestamp(buf: &mut Vec<u8>, field: u32, seconds: i64, nanos: i32) {
    let mut inner = Vec::new();
    if seconds != 0 {
        put_tag(&mut inner, 1, WIRE_VARINT);
        put_varint(&mut inner, seconds as u64);
    }
    if nanos != 0 {
        put_tag(&mut inner, 2, WIRE_VARINT);
        put_varint(&mut inner, nanos as u64);
    }
    put_tag(buf, field, WIRE_LEN);
    put_varint(buf, inner.len() as u64);
    buf.extend_from_slice(&inner);
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Encode a well-formed ExposureEvent exactly as a minimal external producer
/// would: required fields 1–5 plus platform(6), session_id(7), and
/// assignment_probability(8), in ascending field order.
fn producer_encode(event_id: &str, ts_seconds: i64) -> Vec<u8> {
    let mut buf = Vec::new();
    put_string(&mut buf, 1, event_id);
    put_string(&mut buf, 2, "exp-accel-001");
    put_string(&mut buf, 3, "user-42");
    put_string(&mut buf, 4, "treatment-a");
    put_timestamp(&mut buf, 5, ts_seconds, 0);
    put_string(&mut buf, 6, "web");
    put_string(&mut buf, 7, "sess-9");
    put_double(&mut buf, 8, 0.5);
    buf
}

// ---------------------------------------------------------------------------
// Positive contract: a conforming producer's bytes ingest cleanly
// ---------------------------------------------------------------------------

/// 1. M2 decodes a hand-encoded event and every field lands where the
///    producer put it.
#[test]
fn contract_producer_bytes_decode_to_expected_fields() {
    let ts = now_secs();
    let bytes = producer_encode("evt-ext-001", ts);

    let event = ExposureEvent::decode(bytes.as_slice())
        .expect("M2 must decode a conforming producer's bytes");

    assert_eq!(event.event_id, "evt-ext-001");
    assert_eq!(event.experiment_id, "exp-accel-001");
    assert_eq!(event.user_id, "user-42");
    assert_eq!(event.variant_id, "treatment-a");
    assert_eq!(event.timestamp.as_ref().unwrap().seconds, ts);
    assert_eq!(event.platform, "web");
    assert_eq!(event.session_id, "sess-9");
    assert_eq!(event.assignment_probability, 0.5);
}

/// 2. The decoded event passes M2's ingest validation (the same
///    validate_exposure the service runs before Kafka publish).
#[test]
fn contract_producer_bytes_pass_ingest_validation() {
    let bytes = producer_encode("evt-ext-002", now_secs());
    let event = ExposureEvent::decode(bytes.as_slice()).unwrap();

    validate_exposure(&event)
        .expect("a conforming producer's event must pass M2 ingest validation");
}

/// 3. Producer encoding (ascending field order, no map entries) is
///    byte-identical to prost's canonical encoding — so bytes produced by
///    any conforming stack hash/compare identically downstream.
///
///    Scope: only holds for events without interleaving_provenance — map
///    entry order is unspecified in protobuf, so events carrying the map
///    have no single canonical byte form.
#[test]
fn contract_producer_encoding_matches_prost_canonical_bytes() {
    let ts = now_secs();
    let producer_bytes = producer_encode("evt-ext-003", ts);

    let event = ExposureEvent::decode(producer_bytes.as_slice()).unwrap();
    let mut prost_bytes = Vec::new();
    event.encode(&mut prost_bytes).unwrap();

    assert_eq!(
        producer_bytes, prost_bytes,
        "minimal producer encoding must equal prost canonical encoding"
    );
}

/// 4. event_id is the dedup identity: the same event_id offered twice is a
///    duplicate, regardless of other field differences (retry semantics).
#[test]
fn contract_event_id_is_dedup_identity() {
    let mut dedup = EventDedup::new(10_000, 0.01);

    let first =
        ExposureEvent::decode(producer_encode("evt-retry-1", now_secs()).as_slice()).unwrap();
    assert!(
        !dedup.is_duplicate(&first.event_id),
        "first delivery must not be a duplicate"
    );

    // Retry of the same logical exposure: same event_id, later timestamp.
    let retry =
        ExposureEvent::decode(producer_encode("evt-retry-1", now_secs() + 1).as_slice()).unwrap();
    assert!(
        dedup.is_duplicate(&retry.event_id),
        "a retry carrying the same event_id must dedup"
    );

    let distinct =
        ExposureEvent::decode(producer_encode("evt-retry-2", now_secs()).as_slice()).unwrap();
    assert!(
        !dedup.is_duplicate(&distinct.event_id),
        "a distinct event_id must not dedup"
    );
}

/// 5. Unknown fields from a newer producer schema are tolerated: decode
///    succeeds, known fields are intact, validation passes.
#[test]
fn contract_unknown_fields_tolerated() {
    let mut bytes = producer_encode("evt-ext-fwd", now_secs());
    // A hypothetical future field 999 (varint) appended by a newer producer.
    put_tag(&mut bytes, 999, WIRE_VARINT);
    put_varint(&mut bytes, 1);
    // And a future length-delimited field 1000.
    put_string(&mut bytes, 1000, "future-payload");

    let event = ExposureEvent::decode(bytes.as_slice())
        .expect("unknown fields must not break decoding (proto3 forward compat)");
    assert_eq!(event.event_id, "evt-ext-fwd");
    assert_eq!(event.variant_id, "treatment-a");
    validate_exposure(&event).expect("event with unknown fields must still validate");
}

/// 6. assignment_probability is optional on the wire: a producer that omits
///    field 8 entirely (probability unknown) still ingests — 0.0 means
///    "not set" per the proto contract.
#[test]
fn contract_omitted_probability_accepted() {
    let mut buf = Vec::new();
    put_string(&mut buf, 1, "evt-noprob");
    put_string(&mut buf, 2, "exp-accel-001");
    put_string(&mut buf, 3, "user-42");
    put_string(&mut buf, 4, "control");
    put_timestamp(&mut buf, 5, now_secs(), 0);

    let event = ExposureEvent::decode(buf.as_slice()).unwrap();
    assert_eq!(event.assignment_probability, 0.0);
    validate_exposure(&event)
        .expect("omitted assignment_probability decodes to 0.0 and must validate");
}

// ---------------------------------------------------------------------------
// Negative contract: malformed producer output is rejected, not published
// ---------------------------------------------------------------------------

/// 7. Missing variant_id (field 4) fails validation.
#[test]
fn contract_missing_variant_id_rejected() {
    let mut buf = Vec::new();
    put_string(&mut buf, 1, "evt-novariant");
    put_string(&mut buf, 2, "exp-accel-001");
    put_string(&mut buf, 3, "user-42");
    put_timestamp(&mut buf, 5, now_secs(), 0);

    let event = ExposureEvent::decode(buf.as_slice()).unwrap();
    let err = validate_exposure(&event).unwrap_err();
    assert!(
        err.to_string().contains("variant_id is required"),
        "missing variant_id must be rejected: {err}"
    );
}

/// 8. Missing experiment_id (field 2) fails validation.
#[test]
fn contract_missing_experiment_id_rejected() {
    let mut buf = Vec::new();
    put_string(&mut buf, 1, "evt-noexp");
    put_string(&mut buf, 3, "user-42");
    put_string(&mut buf, 4, "control");
    put_timestamp(&mut buf, 5, now_secs(), 0);

    let event = ExposureEvent::decode(buf.as_slice()).unwrap();
    let err = validate_exposure(&event).unwrap_err();
    assert!(
        err.to_string().contains("experiment_id is required"),
        "missing experiment_id must be rejected: {err}"
    );
}

/// 9. Missing timestamp (field 5) fails validation — producers must stamp
///    exposures at emission time, M2 never backfills.
#[test]
fn contract_missing_timestamp_rejected() {
    let mut buf = Vec::new();
    put_string(&mut buf, 1, "evt-nots");
    put_string(&mut buf, 2, "exp-accel-001");
    put_string(&mut buf, 3, "user-42");
    put_string(&mut buf, 4, "control");

    let event = ExposureEvent::decode(buf.as_slice()).unwrap();
    let err = validate_exposure(&event).unwrap_err();
    assert!(
        err.to_string().contains("timestamp is required"),
        "missing timestamp must be rejected: {err}"
    );
}

/// 10. Stale timestamps (>24h old) are rejected — late replays don't enter
///     the exposures topic. Producers must not buffer beyond the window.
#[test]
fn contract_stale_timestamp_rejected() {
    let stale = now_secs() - 25 * 3600;
    let bytes = producer_encode("evt-stale", stale);

    let event = ExposureEvent::decode(bytes.as_slice()).unwrap();
    let err = validate_exposure(&event).unwrap_err();
    assert!(
        err.to_string().contains("outside ±24h"),
        "a 25h-old timestamp must be rejected: {err}"
    );
}

/// 11. Garbage bytes fail at decode — truncated or non-protobuf payloads
///     never reach validation.
#[test]
fn contract_garbage_bytes_fail_decode() {
    let garbage: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0x01, 0x02, 0x03];
    assert!(
        ExposureEvent::decode(garbage).is_err(),
        "garbage bytes must fail proto decode"
    );

    // Truncated valid message: cut a real encoding mid-field.
    let full = producer_encode("evt-truncated", now_secs());
    let truncated = &full[..full.len() - 3];
    assert!(
        ExposureEvent::decode(truncated).is_err(),
        "truncated bytes must fail proto decode"
    );
}

/// 12. NaN assignment_probability is a producer bug and fails fast at
///     ingest (assert_finite! FAIL-FAST), never reaching Kafka.
#[test]
#[should_panic(expected = "FAIL-FAST")]
fn contract_nan_probability_fails_fast() {
    let mut buf = Vec::new();
    put_string(&mut buf, 1, "evt-nan");
    put_string(&mut buf, 2, "exp-accel-001");
    put_string(&mut buf, 3, "user-42");
    put_string(&mut buf, 4, "control");
    put_timestamp(&mut buf, 5, now_secs(), 0);
    put_double(&mut buf, 8, f64::NAN);

    let event = ExposureEvent::decode(buf.as_slice()).unwrap();
    let _ = validate_exposure(&event);
}
