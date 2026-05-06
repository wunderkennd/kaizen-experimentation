# Streaming Integration: Redpanda vs Confluent Schema Registry

**Status:** Authoritative as of 2026-05-06 (Sprint I.3, Issue #478, Phase 2 of multi-cloud
spec `docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md`).

This guide explains the broker swap shipped in #478 — `confluentinc/cp-zookeeper` +
`confluentinc/cp-kafka` + `confluentinc/cp-schema-registry` collapsed into a single
`docker.redpanda.com/redpandadata/redpanda` container — and the wire-compatibility
caveats we hit while validating it. It is the contract for anyone porting more
streaming code or extending the local stack.

## Why we replaced the Confluent stack

Phase 2 of the multi-cloud spec calls out:

> Redpanda schema registry compatibility — streaming integration test replaces
> `confluentinc/cp-schema-registry` with Redpanda in Docker Compose, runs existing
> Rust/Go Kafka producer/consumer tests. Validates wire compatibility before any
> cloud deployment.

> | Schema Registry | Confluent CP container on ECS | Redpanda built-in registry |

Redpanda exposes a Schema Registry HTTP listener inside the broker process itself,
so the three-container Confluent setup collapses to one. This file documents what
we proved by doing it locally, before any cloud-side Redpanda work begins.

## What changed in `docker-compose.yml`

**Removed:**
- `zookeeper:` (`confluentinc/cp-zookeeper:7.7.0`) — Redpanda is Raft-only, no ZK.
- `schema-registry:` (`confluentinc/cp-schema-registry:7.7.0`) — replaced by the
  built-in registry.

**Replaced:**
- `kafka:` — now `docker.redpanda.com/redpandadata/redpanda:v24.3.7`, dev-container
  mode, single broker. Same external port 9092, same in-network port 29092, same
  Schema Registry port 8081. A `schema-registry` network alias keeps the existing
  `kafka-ui` and any future internal consumer working without a config edit.
- `kafka-init:` — now uses `rpk topic create` against `kafka:29092` instead of
  `kafka-topics --create`. Topic names, partition counts, and replication factors
  are unchanged. Idempotent across `up` cycles via `|| true`.

**Unchanged:**
- `postgres`, `redis`, `kafka-ui` (the existing `redpandadata/console:latest` image
  was already Redpanda-aware and points at `kafka:29092` + `http://schema-registry:8081`).
- `docker-compose.test.yml` swaps the broker the same way for CI integration tests.

## Schema Registry HTTP wire compatibility

We verified the four endpoints any Confluent-shaped client touches resolve and
return Confluent-shaped JSON:

| Endpoint            | HTTP | Response                                     | Notes |
|---------------------|------|----------------------------------------------|-------|
| `GET /subjects`     | 200  | `[]` (empty initially)                       | Same as Confluent |
| `GET /config`       | 200  | `{"compatibilityLevel":"BACKWARD"}`          | Default matches Confluent's BACKWARD |
| `GET /mode`         | 200  | `{"mode":"READWRITE"}`                       | Default matches Confluent |
| `GET /schemas/types`| 200  | `["JSON","PROTOBUF","AVRO"]`                 | Same three formats Confluent supports |

The Schema Registry stores its state in a Kafka topic named `_schemas`, which
appears in `rpk topic list` after the broker comes up — same convention Confluent
uses. We don't seed any subjects today; producers will register on first publish
once we wire schema validation into M2.

### Caveats vs Confluent SR

- **Single HTTP listener only.** Confluent's `cp-schema-registry` lets you bind
  multiple `SCHEMA_REGISTRY_LISTENERS`. Redpanda's `--schema-registry-addr` accepts
  a comma-separated list, but **two listeners on the same `0.0.0.0:port` will
  collide with `EADDRINUSE` inside the container.** Use a single listener per
  port, or different ports per listener name. (Hit during initial bring-up:
  `posix_listen failed for address 0.0.0.0:8081: Address already in use`.)
- **rpk `--set` flag form.** Use `--set KEY=VALUE` as **two separate args**, not
  `--set=KEY=VALUE`. The `=`-fused form leaks to the underlying `redpanda` binary
  unparsed and exits the broker with `unrecognised option`. Compose YAML form:
  ```yaml
  - --set
  - redpanda.auto_create_topics_enabled=false
  ```
- **Pandaproxy.** Redpanda always starts a Pandaproxy listener (HTTP-to-Kafka
  bridge) when configured. We expose 18082 host-side for parity but no service
  uses it today; remove the publish if you want the port free.

## Kafka producer/consumer wire compatibility

Both Rust (`rdkafka`) and Go (`segmentio/kafka-go`) clients connect, produce,
consume, and group-coordinate against Redpanda with **zero application-code
changes**. The wire protocol is genuinely the same.

What we verified end-to-end against Redpanda:

| Suite                                                    | Result    | Notes |
|----------------------------------------------------------|-----------|-------|
| `cargo test -p experimentation-ingest` (101 tests)       | **PASS**  | Proto encode/decode roundtrip simulating Kafka — no broker calls. Confirms ingest validation + dedup logic is broker-agnostic. |
| `cargo test -p experimentation-pipeline` (non-ignored, 17 tests) | **PASS**  | Proto contract tests for `RewardEvent` / `ExposureEvent` / `MetricEvent` / `QoEEvent`. |
| `services/metrics/...` (`-tags=integration`)             | **PASS**  | All 10 packages, including `alerts.TestKafkaPublisher_Integration`, `TestKafkaPublisher_MultipleAlerts_Integration`, and `TestM3M5_GuardrailAlertKafkaRoundTrip` — full produce-and-consume against Redpanda over `kafka-go`. |
| `services/orchestration/...` (`-tags=integration`)       | **PASS**  | M2 orchestrator tests (handler, querylog). |
| `services/management/...` (`-tags=integration`)          | **PARTIAL** | 8/12 packages pass against Redpanda. The 4 failures (`handlers`, `sequential`, `store`, `validation`) are **pre-existing on `main`** — they fail to compile with `module gen/go ... but does not contain package experimentation/management/v1` because `gen/go/` only has `experimentation/metrics/` checked in. Verified by re-running on the unchanged Confluent stack via `git stash`; same compilation errors. Not a Redpanda regression and not in scope for #478. Filed as follow-up. |

## Known compatibility caveats and how they show up in our tests

### librdkafka GroupCoordinator + macOS/Colima IPv6 — flaky regardless of broker

**Symptom.** The Rust `--ignored` Kafka tests in
`crates/experimentation-pipeline/tests/m2_m3_event_contract.rs` and
`crates/experimentation-pipeline/tests/reward_consumer_integration.rs` panic with:

```
consumer recv error: KafkaError (Message consumption error:
  BrokerTransportFailure (Local: Broker transport failure))
```

The `librdkafka` debug log shows:

```
%3|...|FAIL|rdkafka#consumer-2| [thrd:GroupCoordinator]:
  GroupCoordinator: localhost:9092: Connect to ipv6#[::1]:9092
  failed: Connection refused (after 0ms in state CONNECT)
```

**Root cause.** macOS resolves `localhost` to both `::1` (IPv6) and `127.0.0.1`
(IPv4). Docker via Colima publishes the broker port on both stacks, but the IPv6
forwarding path on Colima refuses the connection where the IPv4 path doesn't.
`rdkafka`'s **bootstrap thread** retries IPv6 → IPv4 transparently; the
**GroupCoordinator thread** does not, and surfaces the IPv6 RST as a fatal
`BrokerTransportFailure` to `consumer.recv()`.

**Confirmed pre-existing, not Redpanda-introduced.** Reverted the broker to
`confluentinc/cp-kafka:7.7.0` via `git stash` and ran the same single test on a
fresh broker. Same `[thrd:GroupCoordinator]: ... Connect to ipv6#[::1]:9092
failed` line, same panic. (See test session 2026-05-06 17:46 UTC for the side-by-side.)

**Mitigations applied in this PR.**
1. The Redpanda compose advertises the external listener as `127.0.0.1:9092`
   instead of `localhost:9092`. The broker still binds `0.0.0.0:9092` (so
   `bootstrap.servers=localhost:9092` keeps working), but the metadata response
   directs follow-up connections (group coordinator, fetch, produce) at the IPv4
   address directly — no DNS round-trip through `getaddrinfo`. This makes the
   GroupCoordinator path IPv4-only and removes the [::1] hop entirely on macOS.
2. The broker healthcheck uses `rpk cluster health -X brokers=localhost:29092`
   (the in-network listener), which never traverses the host's IPv6 stack.

**What the Redpanda swap does NOT fix.** The bootstrap socket *itself* still
prefers IPv6 first because the test code uses `bootstrap.servers=localhost:9092`.
You'll continue to see one IPv6 RST line per producer/consumer at startup — it's
informational; rdkafka recovers. The flake is the GroupCoordinator path, and
that's now pinned to IPv4 by the advertised address.

**Long-term fix (out of scope for #478).** Either:
- Change test code to `bootstrap.servers=127.0.0.1:9092` (forbidden here:
  acceptance criterion is "no application-code edits"), or
- Set `broker.address.family=v4` in the test client config (also app-code edit), or
- Pin Colima networking to IPv4-only on macOS.

Track via the `--ignored` Rust integration tests; they're not in CI today (only
`experimentation-stats bootstrap_coverage` runs `--ignored` per `.github/workflows/nightly.yml`).
If we promote them to CI later, do the test-side fix first.

### Test-isolation flakiness on shared topics (independent issue)

The same `--ignored` tests ALL share the `exposures` / `metric_events` /
`reward_events` topics, all use `auto.offset.reset=earliest`, and each test
generates a unique `group_id`. On a re-run against a non-empty topic, a new group
reads from offset 0 and consumes a message published by an earlier test, so the
content assertion fails:

```
assertion `left == right` failed
  left: "exp-offset-test"   # from a prior test's payload
 right: "exp-key-contract-42"  # what this test just produced
```

This is a pre-existing test-design issue (per-test topic isolation would fix it).
Also not in scope for #478 — listed here so future debugging doesn't spend
cycles on it.

## How to bring the new stack up

```bash
just infra                     # docker compose up -d --wait
docker compose ps              # all services should be `running healthy`
docker exec kaizen-redpanda rpk cluster info     # smoke check
docker exec kaizen-redpanda curl -sS http://localhost:8081/subjects
```

Tear down with `just infra-reset` (drops volumes too).

## How to extend

- **Adding a topic:** add an `rpk topic create` line to the `kafka-init` command
  in `docker-compose.yml`. Match partition count to the spec in
  `kafka/topic_configs.sh`.
- **Adding a Kafka client in a new service:** point at `kafka:29092` from inside
  the Docker network, or `localhost:9092` from the host. Schema Registry is
  `http://schema-registry:8081` (in-network, via the network alias) or
  `http://localhost:8081` (host).
- **Pinning a different Redpanda version:** edit both `docker-compose.yml` and
  `docker-compose.test.yml`. Keep them on the same tag — CI uses the test file.

## References

- Issue: [#478](https://github.com/wunderkennd/kaizen-experimentation/issues/478)
- Spec: `docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md`
  (Phase 2 + Testing Strategy → Streaming integration test)
- Redpanda Schema Registry docs: <https://docs.redpanda.com/current/manage/schema-reg/schema-reg-overview/>
- Redpanda dev-container mode: <https://docs.redpanda.com/current/get-started/quick-start/>
- librdkafka bootstrap behavior: <https://github.com/confluentinc/librdkafka/blob/master/INTRODUCTION.md#broker-version-compatibility>
