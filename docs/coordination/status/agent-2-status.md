# Agent-2 Status — Phase 5

**Module**: M2 Pipeline
**Last updated**: 2026-03-23

## Current Sprint

Sprint: 5.0
Focus: ADR-021 ModelRetrainingEvent ingestion
Branch: work/cool-wolf

## In Progress

_None (PR submitted)._

## Completed (Phase 5)

- [x] ADR-021 Phase 1: ModelRetrainingEvent ingestion
  - Proto: `IngestModelRetrainingEvent` RPC added to `pipeline_service.proto`
  - Kafka: `model_retraining_events` topic (8 partitions) in `kafka/topic_configs.sh` and `docker-compose.yml`
  - Validation: `validate_model_retraining_event()` in `experimentation-ingest` — enforces `model_id`, `training_data_start`, `training_data_end` required; validates window ordering; skips ±24h restriction for historical training windows
  - Bloom filter dedup wired through shared `EventDedup` (keyed by `event_id`)
  - Service: `ingest_model_retraining_event` handler in `experimentation-pipeline/src/service.rs`
  - Contract test (M2→M3 wire format): `test_model_retraining_event_kafka_roundtrip_serialization` + `test_model_retraining_event_routes_to_correct_topic`
  - 49 tests in `experimentation-ingest`, 43 in `experimentation-pipeline` — all green

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
- Dedup: Bloom filter on `event_id` (shared with all other event types)
- Retention: 180 days (longer than other topics — historical analysis needed)
