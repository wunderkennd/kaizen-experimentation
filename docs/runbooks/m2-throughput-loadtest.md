# Runbook: M2 Throughput Test — 100K events/sec via Redpanda

**Validates**: issue #502 (Phase 4 Validation, `docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md`) — the headline throughput SLA: M2 Pipeline sustains **100K events/sec for 5 minutes** through Redpanda on the GCP stack, with **zero message loss** and **bounded downstream consumer lag**.

## How it works

```
                    ┌─ 1. ingest acceptance ────────────┐
k6 batch gRPC ────▶ M2 ingest (:50052) ────▶ Redpanda topics ────▶ consumers
(loadtest_m2_       IngestBatchResponse      │                      │
 throughput.js)     accounting               ├─ 2. producer offset  ├─ 3. consumer lag
                                             │    advance (rpk)     │    (rpk group describe)
                                             └──── m2_throughput_watch.py sample ────┘
                                                              │
                                              m2_throughput_watch.py evaluate  → PASS/FAIL
```

| Piece | File | Role |
| --- | --- | --- |
| Generator | `scripts/loadtest_m2_throughput.js` | k6 gRPC at `TARGET_EPS` events/sec: `IngestExposureBatch` 40% / `IngestMetricEventBatch` 30% / `IngestQoEEventBatch` 15% / unary `IngestRewardEvent` 15% (no batch RPC exists for rewards). Unique `event_id`s so Bloom dedup never eats synthetic events. |
| Watcher | `scripts/m2_throughput_watch.py sample` | Every `SAMPLE_INTERVAL` seconds, sums `HIGH-WATERMARK` across all partitions of the four event topics and total lag of each consumer group, via `rpk` (text output, version-stable). |
| Gate | `scripts/m2_throughput_watch.py evaluate` | Combines both into the verdict (below). Exit 0 = PASS. |
| Orchestrator | `scripts/loadtest_m2_throughput.sh` | Preflight → sampler → k6 (warmup + steady) → drain → gate. |
| Offline tests | `scripts/test_m2_throughput_gate.sh` | Parser + verdict tests, no cluster needed (`just test-m2-throughput-gate`). |

## Pass/fail gate

| Check | Criterion (defaults) | Why this threshold |
| --- | --- | --- |
| `sustained_throughput` | Offset advance across `exposures` + `metric_events` + `reward_events` + `qoe_events` ≥ `TARGET_EPS` (100 000 ev/s) over the steady window, **and** every `SAMPLE_INTERVAL` window ≥ `BUCKET_FLOOR` (0.95) × target | Measured at Redpanda (the source of truth), not the client — catches ingest buffering stalls that client-side accounting hides. The 5% floor tolerates sampling jitter while rejecting real dips. |
| `zero_message_loss` | Every k6-accepted event advanced an offset: `offset_advance ≥ accepted`, and `invalid == 0` | M2 acks after validation/dedup but publishes async (crash-only buffer) — this check proves accepted events actually reached Redpanda by end of drain. |
| `consumer_lag_bounded[g]` | Peak total lag per group ≤ `LAG_THRESHOLD` (100 000 msgs) during load + drain | **Parity with the platform's own SLO**: the `PipelineConsumerLag` Prometheus alert (`monitoring/prometheus/alerts.yml`) warns at `kafka_consumer_group_lag > 100000`. A run that would page operators is not a pass. At the reward share (15K ev/s) 100K messages ≈ 6.7 s of stream — the consumer stays seconds, not minutes, behind head. |
| `generator_health` | k6 `dropped_iterations == 0`, gRPC error rate < 0.1% | If k6 could not offer the load, the other checks are vacuous. |

**Redpanda Cloud sizing note.** At ~250 B/event serialized, 100K ev/s ≈ 25 MB/s produce (before replication). The dev cluster tier (`redpanda:throughputTier`, e.g. `tier-1-*` per `infra/pkg/streaming/redpanda.go`) is at/near that envelope — check the current [Redpanda Cloud tier limits](https://docs.redpanda.com/redpanda-cloud/reference/tiers/) for the exact tier before a run; prod tiers have ample headroom. If the tier is undersized the run fails honestly at `sustained_throughput` — raise the tier, not the thresholds. `LAG_THRESHOLD` stays at alert parity regardless of tier; override only alongside a matching alert change.

## Running against the GCP dev stack

The dev Redpanda cluster uses **private connectivity** (`connectionType: private`) — run from a host inside the tenant VPC (bastion / GCE utility VM), not from a laptop or GitHub runner.

```bash
# 1. Connection details from the Pulumi stack (infra/, dev stack)
cd infra
BROKERS=$(pulumi stack output redpandaBootstrapBrokers --stack dev)
KAFKA_SASL_USER=$(pulumi config get redpanda:kafkaUsername --stack dev)
KAFKA_SASL_PASS=$(pulumi config get redpanda:kafkaPassword --stack dev)  # secret

# 2. M2 ingest endpoint (deployed by #489). In-VPC address, gRPC port 50052.
#    For a TLS-terminated endpoint use PLAINTEXT=false and the :443 host.
PIPELINE_ADDR="<m2-ingest-host>:50052"

# 3. Full SLA run (5 min steady state + 30 s warmup + 60 s drain)
cd ..
BROKERS="$BROKERS" KAFKA_SASL_USER="$KAFKA_SASL_USER" KAFKA_SASL_PASS="$KAFKA_SASL_PASS" \
KAFKA_TLS_ENABLED=1 PIPELINE_ADDR="$PIPELINE_ADDR" REQUIRE_GROUPS=1 \
  just loadtest-m2-throughput
```

Prerequisites on the runner host: `k6`, `rpk` (≥ 23.x; [install](https://docs.redpanda.com/current/get-started/rpk-install/)), `python3`. Auth is SASL/SCRAM-SHA-512 + TLS, matching cluster provisioning.

**Quiesce first.** The zero-loss check compares k6-accepted counts against topic offset advance; concurrent producers inflate offsets and can mask real loss (the gate warns when `advance > accepted`). Pause other event sources on dev for the ~7-minute window.

**Consumer groups.** Default measured group: `bandit-policy-service` (M4b ← `reward_events`). Add groups with `CONSUMER_GROUPS=a,b`. On a stack where the consumer fleet is running, set `REQUIRE_GROUPS=1` so an unobservable group fails instead of warning — that is the acceptance configuration for #502.

**Record the run** (acceptance follow-through): attach `/tmp/m2t-<ts>/gate_report.json` plus the console report to issue #502.

### Local smoke (no GCP)

```bash
just infra          # local Kafka via docker compose
# start the pipeline binary as in scripts/loadtest_pipeline.sh, then:
just loadtest-m2-throughput-smoke
```

Smoke mode (2K ev/s × 30 s, relaxed floor) validates wiring only — it says nothing about the SLA.

## Knobs

| Env | Default | Meaning |
| --- | --- | --- |
| `TARGET_EPS` / `DURATION` / `WARMUP` / `DRAIN_WAIT` | 100000 / 300 / 30 / 60 | Rate (events/s) and phase lengths (s) |
| `BATCH_SIZE` | 100 | Events per batch RPC |
| `LAG_THRESHOLD` / `BUCKET_FLOOR` | 100000 / 0.95 | Gate thresholds (see table above) |
| `TOPICS` / `CONSUMER_GROUPS` / `REQUIRE_GROUPS` | four event topics / `bandit-policy-service` / 0 | Measurement scope |
| `PIPELINE_ADDR` / `PLAINTEXT` / `BROKERS` / `KAFKA_SASL_USER` / `KAFKA_SASL_PASS` / `KAFKA_TLS_ENABLED` | local dev values | Endpoints + auth |

## CI wiring

Offline pieces run anywhere: `just test-m2-throughput-gate` (gate/parser tests) and `k6 inspect scripts/loadtest_m2_throughput.js` (parse check). The full run is **not** CI-runnable — GitHub runners have no route to the private dev VPC; it stays an operator/bastion procedure (above).

`nightly-loadtest.yml` should pick up both offline pieces. That workflow edit could not be shipped from the #502 worker session (the GitHub App token lacks `workflows` permission); apply this diff in a follow-up commit:

```diff
       - name: Validate k6 scripts parse
         run: |
-          for script in scripts/loadtest_assignment.js scripts/loadtest_policy.js scripts/loadtest_flags.js; do
+          for script in scripts/loadtest_assignment.js scripts/loadtest_policy.js scripts/loadtest_flags.js \
+                        scripts/loadtest_pipeline.js scripts/loadtest_m2_throughput.js; do
             if [ -f "$script" ]; then
               echo "::group::Validating $script"
               k6 inspect "$script" || { echo "::error::$script failed to parse"; exit 1; }
               echo "::endgroup::"
             fi
           done
+
+      - name: M2 throughput gate — offline unit tests
+        run: bash scripts/test_m2_throughput_gate.sh
```

(`loadtest_pipeline.js` was missing from the parse list; the diff fixes that too.)
