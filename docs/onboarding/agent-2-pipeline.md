# Agent-2 Quickstart: M2 Event Pipeline (Rust + Go)

## Your Identity

| Field | Value |
|-------|-------|
| Module | M2: Event Ingestion & Pipeline |
| Languages | Rust (ingestion/validation) + Go (orchestration, SQL query logging) |
| Crates you own | `experimentation-ingest` (library), `experimentation-pipeline` (binary) |
| Go packages you own | `services/orchestration/` |
| Proto package | `experimentation.pipeline.v1` |
| Infra you own | Kafka producer config, Bloom filter for dedup |
| Primary SLA | p99 < 10ms ingest latency, zero data loss (at-least-once), recovery < 2 seconds |

## Read These First (in order)

1. **Design doc v5.1** — Sections 2.2 (crash-only design), 2.4 (fail-fast data integrity), 5 (M2 specification)
2. **ADR-001** (language selection — why Rust for ingestion)
3. **Proto files** — `pipeline_service.proto`, `event.proto`, `qoe.proto`
4. **Kafka topic configs** — `kafka/topic_configs.sh` (partition counts, retention, consumer groups)
5. **Mermaid diagram** — `data_flow.mermaid` (your position as the data entry point)

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| Client SDKs | Send exposure, metric, reward, QoE events | No — generate synthetic events for testing. |
| M1 (Agent-1) | Exposure events include assignment metadata | No — your job is validation and Kafka publishing; you don't compute assignments. |

## Who Depends on You (downstream)

| Module | What they need from you | Impact if you're late |
|--------|------------------------|----------------------|
| M3 (Agent-3) | Events on Kafka topics (exposures, metric_events, qoe_events) | **Critical** — M3 has nothing to compute without events. |
| M4b (Agent-4) | Reward events on `reward_events` topic | Bandit policy can't learn without rewards. |
| M5 (Agent-5) | Guardrail alerts on `guardrail_alerts` topic (published by M3, but sourced from your events) | Indirect dependency. |
| Delta Lake | All events land in Delta Lake tables via Kafka Connect | Data warehouse empty without you. |

## Your First PR: Event Validation + Kafka Publisher

**Goal**: Accept an `IngestExposure` RPC, validate the event, and publish to the `exposures` Kafka topic.

```
crates/experimentation-ingest/
├── Cargo.toml
├── src/
│   ├── lib.rs           # Public API: validate_exposure, validate_metric, etc.
│   ├── validation.rs    # Schema validation, required fields, timestamp bounds
│   └── dedup.rs         # Bloom filter for event_id deduplication
crates/experimentation-pipeline/
├── Cargo.toml
├── src/
│   └── main.rs          # tonic gRPC server, Kafka producer (rdkafka)
```

**Acceptance criteria**:
- `IngestExposure` with valid event → published to `exposures` topic, response `accepted: true`.
- `IngestExposure` with missing `experiment_id` → rejected, `accepted: false`, error detail in gRPC status.
- Duplicate `event_id` → `accepted: false` (Bloom filter hit). False positive rate < 0.1%.
- Crash (kill -9) during Kafka publish → on restart, no duplicate events (Kafka idempotent producer config).

**Why this first**: You are the data entry point for the entire platform. Every module downstream is starved without events flowing. A minimal working pipeline unblocks Agent-3 (metrics), Agent-4 (bandit rewards), and the Delta Lake sink.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] `experimentation-ingest` crate stub with validation function signatures
- [ ] `experimentation-pipeline` binary stub (tonic server + rdkafka producer)
- [ ] Kafka producer config with idempotent writes (`enable.idempotence=true`)

### Phase 1 (Weeks 2–7)
- [ ] `IngestExposure` / `IngestExposureBatch` RPCs
- [ ] `IngestMetricEvent` / `IngestMetricEventBatch` RPCs
- [ ] `IngestRewardEvent` RPC
- [ ] `IngestQoEEvent` / `IngestQoEEventBatch` RPCs
- [ ] Bloom filter deduplication (crate: `bloomfilter` or custom scalable Bloom)
- [ ] Event validation: required fields, timestamp within ±24h of server time, valid enum values
- [ ] Protobuf serialization for Kafka values (not JSON — schema registry compatible)
- [ ] Go orchestration layer: SQL query logging for M3 transparency, health endpoints
- [ ] Load test: sustain 100K events/sec with p99 < 10ms

### Phase 2 (Weeks 6–11)
- [ ] Backpressure handling: if Kafka producer queue is full, return gRPC RESOURCE_EXHAUSTED
- [ ] Metrics: events_accepted_total, events_rejected_total, events_deduplicated_total, kafka_publish_latency_ms
- [ ] Graceful degradation: if Kafka is unreachable, buffer to local disk (bounded, drop oldest on overflow)

### Phase 3 (Weeks 10–17)
- [ ] QoE event enrichment: validate PlaybackMetrics field ranges (e.g., rebuffer_ratio ∈ [0, 1])
- [ ] Interleaving provenance validation: verify provenance map keys match merged list items

### Phase 4 (Weeks 16–22)
- [ ] Chaos engineering: kill -9 under 100K events/sec load, verify recovery < 2 seconds, zero data loss
- [ ] Kafka partition rebalance testing: consumer group changes don't cause event loss
- [ ] End-to-end latency tracing: event ingestion → Kafka → M3 consumption < 30 seconds

## Local Development

```bash
# Start local infra
docker-compose up -d kafka zookeeper schema-registry

# Build and test Rust ingestion library
cargo test --package experimentation-ingest

# Run the pipeline server
KAFKA_BROKERS=localhost:9092 cargo run --package experimentation-pipeline

# Send a test event
grpcurl -plaintext -d '{
  "event": {
    "event_id": "evt_001",
    "experiment_id": "exp_001",
    "user_id": "user_123",
    "variant_id": "var_a",
    "timestamp": "2026-03-03T12:00:00Z"
  }
}' localhost:50051 experimentation.pipeline.v1.EventIngestionService/IngestExposure

# Verify it landed on Kafka
kafka-console-consumer --bootstrap-server localhost:9092 --topic exposures --from-beginning
```

## Testing Expectations

- **Unit tests**: >90% coverage on validation logic. Test every rejection path (missing field, bad timestamp, invalid enum).
- **Integration**: Docker Compose with Kafka. Round-trip test: ingest event → consume from topic → deserialize → verify fields match.
- **Load/stress**: Use `ghz` or custom Rust load generator. Sustained 100K events/sec for 5 minutes. Monitor Kafka producer queue depth, GC pauses (none — it's Rust), and p99 latency.
- **Chaos**: kill -9 during sustained load. Restart. Verify event count in Kafka matches expected (no loss, no unintended duplicates beyond Bloom filter false positives).

## Common Pitfalls

1. **Kafka producer batching**: Default rdkafka `queue.buffering.max.ms` is 5ms. For lowest latency, set to 0 (immediate send). For throughput, keep default. Choose based on SLA priority.
2. **Bloom filter sizing**: Size for expected event volume (100M events/day). At 0.1% FPR, this requires ~120MB of memory. Reset daily or use a scalable (rotating) Bloom filter.
3. **Protobuf timestamp precision**: `google.protobuf.Timestamp` uses seconds + nanos. Client SDKs may send millisecond-precision timestamps. Validate but don't reject on precision differences.
4. **Event ordering**: Kafka guarantees ordering within a partition. Key events by the appropriate field (see topic_configs.sh). Exposure events are keyed by experiment_id; metric events by user_id.
5. **Schema evolution**: Never remove or rename a proto field. Add new optional fields only. buf breaking CI catches this, but be aware when evolving event schemas.
