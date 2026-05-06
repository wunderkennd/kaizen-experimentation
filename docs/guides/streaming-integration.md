# Streaming Integration Guide

**Audience:** developers writing Kafka producer/consumer code or schema-aware
clients against the local dev stack and CI test infrastructure.

**Status:** Phase 2 of the multi-cloud migration — Redpanda has replaced the
Confluent CP stack in `docker-compose.yml` and `docker-compose.test.yml`. All
existing Rust and Go Kafka clients continue to work without code changes.
See `docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md` for the
full design context.

## What changed

| Concern | Before (Confluent CP 7.7.0) | After (Redpanda v24.3.5) |
| --- | --- | --- |
| Containers | `confluentinc/cp-zookeeper` + `confluentinc/cp-kafka` + `confluentinc/cp-schema-registry` + a `kafka-init` job | Single `redpanda` container + a small `redpanda-init` topic-seeding job |
| Process model | JVM (Kafka, Zookeeper, SR) | Single C++ process |
| Kafka API port (host) | `localhost:9092` | `localhost:9092` (unchanged) |
| Kafka API port (in-network) | `kafka:29092` | `redpanda:29092` |
| Schema Registry port | `localhost:8081` | `localhost:8081` (unchanged) |
| Schema Registry storage | `_schemas` topic on Kafka | Internal Raft group (managed by Redpanda) |
| Admin tooling | `kafka-topics`, `kafka-console-consumer` | `rpk topic`, `rpk cluster`, `rpk registry` |
| Default replication factor | 1 (dev) | 1 (`dev-container` mode) |
| Health check | `kafka-broker-api-versions` | `rpk cluster health` |
| UI | `kafka-ui` (Redpanda Console) → `kafka:29092` + `http://schema-registry:8081` | Same image, repointed at `redpanda:29092` + `http://redpanda:8081` |

The Kafka wire protocol, Schema Registry HTTP API contract, port numbers, and
container DNS surface stay the same from the application's perspective.
Redpanda is wire-compatible with Kafka 3.x and ships a Confluent-compatible
Schema Registry. Tests that connected to `localhost:9092` and `localhost:8081`
continue to do so without edits.

## How to use

### Bring up the stack

```bash
docker compose up -d                  # starts redpanda, redpanda-init, postgres, redis, kafka-ui
just infra                            # justfile shortcut (--wait gates on health)
```

The `redpanda-init` service runs `rpk topic create` for the seven topics the
platform expects (`exposures`, `metric_events`, `reward_events`, `qoe_events`,
`guardrail_alerts`, `model_retraining_events`, `model_training_requests`),
exits, and stays exited. This mirrors the previous `kafka-init` Confluent job.

### Inspect topics and consumer groups

Inside the broker container (preferred):

```bash
docker exec -it redpanda rpk topic list --brokers=localhost:9092
docker exec -it redpanda rpk topic describe metric_events --brokers=localhost:9092
docker exec -it redpanda rpk group list --brokers=localhost:9092
docker exec -it redpanda rpk topic consume reward_events --brokers=localhost:9092 -o end
```

From the host, with `rpk` installed (`brew install redpanda-data/tap/redpanda`):

```bash
rpk topic list -X brokers=localhost:9092
rpk registry subject list -X registry.hosts=localhost:8081
```

`rpk` is the only CLI you should reach for. `kafka-topics`/`kafka-console-*`
are not present on the Redpanda image; if you have a habit of typing them,
update muscle memory now.

### Schema Registry

The HTTP API is served by the broker itself on `:8081`. There is no separate
container, and there is no `_schemas` Kafka topic — the registry stores
schemas in its own Raft group. From the application's perspective this is
invisible; clients hit the same endpoints they used against Confluent.

```bash
curl -s http://localhost:8081/subjects
curl -s http://localhost:8081/schemas/types
curl -s http://localhost:8081/config
```

#### Compatibility caveats

The Confluent and Redpanda implementations are intentionally interchangeable
for the endpoints typical clients use, but a few edges differ. None of them
are exercised by the current Rust/Go test suite — this section exists so
future schema-validation work doesn't get surprised.

| Surface | Confluent CP 7.x | Redpanda v24.3 | Impact on this repo |
| --- | --- | --- | --- |
| Subject CRUD (`/subjects`, `/subjects/{n}/versions`) | ✓ | ✓ | None — wire-compatible |
| Schema lookup (`/schemas/ids/{id}`, `/schemas/types`) | ✓ | ✓ | None |
| Compatibility checks (`/compatibility/...`) | ✓ | ✓ | None |
| `/config` (global + per-subject) | ✓ | ✓ | None — `BACKWARD` is the default in both |
| `/mode` (global + per-subject) | ✓ | ✓ | None |
| `/exporters` (Schema Linking) | ✓ | ✗ — not implemented | Out of scope for Kaizen; we don't replicate schemas across clusters in dev/test |
| `_schemas` topic config tweaks (e.g. `kafkastore.topic`) | Required env wiring on the SR container | N/A — Redpanda stores schemas internally | Drop any `SCHEMA_REGISTRY_KAFKASTORE_*` env vars when porting tooling |
| JSON Schema draft support | drafts 4, 6, 7 | drafts 4, 6, 7, 2019-09, 2020-12 | Strictly additive |
| Protobuf normalization | Confluent canonical form | Same | Identical — no diff for our `.proto`-derived schemas |
| Per-subject naming strategy headers | Required when not `TopicNameStrategy` | Same | None — we use `TopicNameStrategy` everywhere |

If you add a Schema Registry-aware client and need to talk to the registry
from inside a container, the URL is `http://redpanda:8081` (not
`http://schema-registry:8081`).

### Kafka client configuration

No change. Existing producer/consumer config keeps working:

```rust
// Rust — rdkafka
ClientConfig::new()
    .set("bootstrap.servers", "localhost:9092")
    .set("group.id", "metric-consumer")
```

```go
// Go — segmentio/kafka-go
kafka.NewReader(kafka.ReaderConfig{
    Brokers: []string{"localhost:9092"},
    Topic:   "guardrail_alerts",
    GroupID: "m5-consumer",
})
```

Inside the docker network, use `redpanda:29092` instead of `localhost:9092`
(replacing the previous `kafka:29092`). For services started via Pulumi/IaC
this is wired automatically through the `BootstrapBrokers` output.

## CI and local test workflow

`docker-compose.test.yml` boots the same Redpanda image with `tmpfs`-backed
storage and no `kafka-ui`. Tests gated by `#[ignore]` (Rust) and
`//go:build integration` (Go) connect to `localhost:9092` exactly as before.

```bash
docker compose -f docker-compose.test.yml up -d --wait

# Rust — runs the live-Kafka tests
cargo test -p experimentation-pipeline -- --ignored
cargo test -p experimentation-ingest   -- --ignored

# Go — runs the integration-tagged tests
go test -tags=integration ./services/metrics/...
```

### Why "no application code changes" matters

Phase 2 is the wire-compatibility gate. A successful run of the integration
suites against Redpanda — with no edits to `crates/`, `services/`, or
`sdks/` — is the prerequisite for any cloud-side Redpanda deployment
(`infra/pkg/streaming/redpanda.go`, Phase 5+ work). If a test ever needs a
code edit to pass against Redpanda, that's a bug in either the swap or the
client config; surface it before the cloud migration consumes it.

## Migration checklist for new tooling

When you add new code that touches Kafka or the Schema Registry:

- [ ] Talk to brokers via `localhost:9092` from the host or `redpanda:29092`
      from inside docker. Don't hardcode `kafka:29092`.
- [ ] Talk to the registry via `localhost:8081` / `redpanda:8081`. Don't
      hardcode `schema-registry:8081`.
- [ ] If you need a topic that doesn't exist yet, add it to the
      `redpanda-init` service in **both** compose files in the same PR.
- [ ] If you start using a Schema Registry endpoint not listed in the
      compatibility table above, run `curl http://localhost:8081/<endpoint>`
      against both Confluent and Redpanda first and update this guide.

## References

- Spec: `docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md`
  (Phase 2 + Testing Strategy → Streaming integration test)
- Redpanda Schema Registry docs:
  https://docs.redpanda.com/current/manage/schema-reg/schema-reg-overview/
- Redpanda `rpk` reference:
  https://docs.redpanda.com/current/reference/rpk/
- ADR-026 (custom metrics) and ADR-027 (TOST equivalence) — both are likely
  consumers of the Schema Registry; link this guide from their design docs
  when they reach implementation.
