# Agent-2 Status — Phase 5

**Module**: M2 Pipeline
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.0
Focus: ADR-021 ModelRetrainingEvent ingestion — composite dedup key + contract tests
Branch: work/happy-otter

## In Progress

_None (PR submitted)._

## Completed (Phase 5)

- [x] ADR-021 Phase 1: ModelRetrainingEvent ingestion
  - Proto: `IngestModelRetrainingEvent` RPC added to `pipeline_service.proto`
  - Kafka: `model_retraining_events` topic (8 partitions) in `kafka/topic_configs.sh` and `docker-compose.yml`
  - Validation: `validate_model_retraining_event()` in `experimentation-ingest` — enforces `model_id`, `training_data_start`, `training_data_end` required; validates window ordering; skips ±24h restriction for historical training windows
  - Service: `ingest_model_retraining_event` handler in `experimentation-pipeline/src/service.rs`

- [x] ADR-021 Phase 1b: Composite dedup key + expanded contract tests
  - Dedup key: `model_retraining_dedup_key()` added to `experimentation-ingest/src/validation.rs`
    - Key format: `"{model_id}:{training_data_start_seconds}"` (composite, not `event_id`)
    - Rationale: same model retrained on same data window is semantically duplicate regardless of caller event_id
  - Pipeline wired: `ingest_model_retraining_event` uses `validation::model_retraining_dedup_key()` for the Bloom filter check
  - New test `test_ingest_model_retraining_composite_key_dedup`: proves events with different `event_id` but same `model_id+training_data_start` are deduped
  - M2→M3 contract test section added to `m2_m3_event_contract.rs` (Section 6):
    - `test_model_retraining_event_roundtrip_all_fields`
    - `test_model_retraining_event_roundtrip_required_fields_only`
    - `test_model_retraining_event_training_window_seconds_preserved`
    - `test_model_retraining_event_active_experiment_ids_repeated_field`
    - `test_model_retraining_event_dedup_key_format`
    - `test_model_retraining_event_decode_garbage_fails`
    - `test_kafka_model_retraining_event_roundtrip` (Kafka roundtrip, `#[ignore]` — requires infra)
  - 54 tests in `experimentation-ingest` — all green
  - 44 tests in `experimentation-pipeline` service unit tests — all green
  - 38 tests in `m2_m3_event_contract.rs` (non-Docker) — all green

## Blocked

_None._

## Next Up

- ADR-021 Phase 2 (if requested): M3 feedback loop contamination SQL pipeline consuming `model_retraining_events` — depends on Agent-3

## Notes for Downstream Consumers (M3)

- Topic: `model_retraining_events`, partitioned by `model_id`, 8 partitions
- Wire format: `experimentation.common.v1.ModelRetrainingEvent` (Protobuf)
- Required fields: `event_id`, `model_id`, `training_data_start`, `training_data_end`
- Optional: `retrained_at`, `active_experiment_ids`, `treatment_contamination_fraction`
- `active_experiment_ids` is populated by the external ML training pipeline at retraining time
- Dedup: Bloom filter on composite key `"{model_id}:{training_data_start_seconds}"` — not `event_id`
- Retention: 180 days (longer than other topics — historical analysis needed)
- `training_data_start` / `training_data_end` epoch seconds survive Kafka roundtrip exactly (verified in contract test)
