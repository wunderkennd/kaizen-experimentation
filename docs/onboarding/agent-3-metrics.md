# Agent-3 Quickstart: M3 Metric Computation Engine (Go)

## Your Identity

| Field | Value |
|-------|-------|
| Module | M3: Metric Computation Engine |
| Language | Go (orchestration) + Spark SQL (computation) + Python (surrogate model training scripts only) |
| Go packages you own | `services/metrics/` |
| Shared libraries | `experimentation-surrogate` (Go + Python) |
| Proto package | `experimentation.metrics.v1` |
| Infra you own | Spark job configs, Delta Lake writes, Redis feature store population, MLflow model registry |
| Primary SLA | Daily metrics complete within 2h of event arrival; guardrail metrics within 30min of hourly trigger |

## Read These First (in order)

1. **Design doc v5.1** — Sections 6 (M3 specification), 2.7 (SQL transparency & notebook export), 1.3 (SVOD-specific capabilities)
2. **Proto files** — `metrics_service.proto`, `metric.proto`, `interleaving.proto`, `surrogate.proto`, `lifecycle.proto`, `qoe.proto`, `event.proto`
3. **Delta Lake tables** — `delta/delta_lake_tables.sql` (you write: metric_summaries, interleaving_scores, content_consumption, daily_treatment_effects)
4. **Kafka topic configs** — `kafka/topic_configs.sh` (you consume: exposures, metric_events, qoe_events; you produce: guardrail_alerts)
5. **PostgreSQL DDL** — `sql/001_schema.sql` (you write: query_log table)
6. **Mermaid diagram** — `data_flow.mermaid` (you are the central transformation layer)

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| M2 (Agent-2) | Events on Kafka topics (exposures, metric_events, qoe_events) | **Yes** — without events, you have nothing to compute. Use synthetic event generators initially. |
| M5 (Agent-5) | Experiment configs and metric definitions (via Management API) | Partially — you need to know which experiments are RUNNING and what metrics to compute. Use local config initially. |
| Redis feature store | User attributes for lifecycle segmentation | No — you populate Redis yourself from subscriber data. |

## Who Depends on You (downstream)

| Module | What they need from you | Impact if you're late |
|--------|------------------------|----------------------|
| M4a (Agent-4) | Delta Lake tables: metric_summaries, interleaving_scores, content_consumption, daily_treatment_effects | **Critical** — M4a reads your output for all analysis. |
| M5 (Agent-5) | Guardrail alerts on `guardrail_alerts` Kafka topic | M5 can't auto-pause without your breach detection. |
| M6 (Agent-6) | Query log entries in PostgreSQL for "View SQL" feature | UI feature delayed; not blocking. |

## Your First PR: Standard Metric Computation Job

**Goal**: A Go service that reads from the `metric_events` Kafka topic (via Delta Lake batch), joins with `exposures`, and produces per-user metric summaries.

```
services/metrics/
├── cmd/
│   └── main.go              # Service entry point, connect-go server
├── internal/
│   ├── jobs/
│   │   ├── standard.go      # Daily metric computation orchestrator
│   │   ├── guardrail.go     # Hourly guardrail metric computation
│   │   └── interleaving.go  # Interleaving score computation
│   ├── spark/
│   │   ├── client.go        # Spark submit / Databricks Jobs API client
│   │   └── templates/       # SQL templates (Go text/template)
│   ├── querylog/
│   │   └── writer.go        # Log every SQL to query_log table
│   └── export/
│       └── notebook.go      # Generate .ipynb from SQL templates
```

**Acceptance criteria**:
- Given 1,000 synthetic exposure events and 10,000 metric events, the job produces correct metric_summaries in Delta Lake.
- Every Spark SQL query is logged to the `query_log` PostgreSQL table with: SQL text, row count, duration, job type.
- `ExportNotebook` RPC returns a valid .ipynb file containing all SQL queries for a given experiment.

**Why this first**: You are the data transformation layer between raw events and statistical analysis. M4a can't compute treatment effects until you've produced metric summaries. The SQL transparency requirement means every query is logged from day one.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] Go module skeleton with connect-go server
- [ ] Spark client stub (Databricks Jobs API or local spark-submit)
- [ ] Query log writer (PostgreSQL insert)

### Phase 1 (Weeks 2–7)
- [ ] Standard metric computation: MEAN, PROPORTION, COUNT metrics from metric_events
- [ ] RATIO metric computation with delta method inputs
- [ ] PERCENTILE metric computation
- [ ] CUPED covariate computation (pre-experiment metric values)
- [ ] Guardrail metric hourly computation
- [ ] Guardrail breach detection → publish GuardrailAlert to Kafka
- [ ] Query log: all SQL logged with experiment_id, metric_id, duration, row count
- [ ] Notebook export: Jupyter .ipynb generation from SQL templates

### Phase 2 (Weeks 6–11)
- [ ] Interleaving score computation: join exposure provenance with metric events, compute per-algorithm credit
- [ ] QoE metric aggregation from qoe_events
- [ ] Content consumption distribution tables (for interference analysis)
- [ ] Daily treatment effect time series (for novelty detection)
- [ ] SQL template validation: all templates produce valid Spark SQL on empty inputs

### Phase 3 (Weeks 10–17)
- [ ] Surrogate metric computation: load MLflow model, compute projections
- [ ] Lifecycle segmentation: classify users from Redis, compute per-segment metrics
- [ ] Surrogate model recalibration trigger
- [ ] CUSTOM metric type: execute user-provided Spark SQL (sandboxed)

### Phase 4 (Weeks 16–22)
- [ ] End-to-end latency validation: daily jobs < 2h, guardrail jobs < 30min
- [ ] Databricks notebook export (in addition to Jupyter)
- [ ] Spark job failure recovery: automatic retry with exponential backoff

## Local Development

```bash
# Start local infra
docker-compose up -d postgres kafka spark-local redis

# Run Go tests
cd services/metrics
go test -race -cover ./...

# Run with local config
POSTGRES_DSN=postgres://localhost/experimentation \
KAFKA_BROKERS=localhost:9092 \
SPARK_MASTER=local[*] \
go run cmd/main.go

# Trigger a metric computation manually
grpcurl -plaintext -d '{"experiment_id": "exp_001"}' \
  localhost:50053 experimentation.metrics.v1.MetricComputationService/ComputeMetrics
```

## Testing Expectations

- **Unit tests**: testify for Go. Test each SQL template produces correct output on known inputs. Mock Spark with local SQL execution (DuckDB or SQLite for unit tests).
- **Integration**: Docker Compose with Spark, Kafka, PostgreSQL. End-to-end: publish events to Kafka → trigger computation → verify Delta Lake output.
- **SQL correctness**: For each metric type, create a "golden" dataset with hand-computed expected values. Assert computed metrics match within floating-point tolerance (1e-6).
- **Query log completeness**: After any computation, assert query_log has an entry for every metric computed.

## Common Pitfalls

1. **Spark SQL vs Go**: You orchestrate Spark jobs from Go — you do NOT write metric computation logic in Go. Spark handles the data-heavy work. Go handles job scheduling, config management, and query logging.
2. **SQL transparency is non-negotiable**: Every single SQL query that touches experiment data must be logged. The "View SQL" feature in M6 depends on this. If a query isn't logged, it doesn't exist from the user's perspective.
3. **Delta method for ratios**: RATIO metrics (e.g., revenue/sessions) require delta method variance estimation. Don't naively compute variance of the ratio — compute the covariance matrix of numerator and denominator sums.
4. **CUPED pre-period**: The covariate must be from BEFORE the experiment started. A common bug is including post-treatment data in the covariate calculation, which biases the estimate.
5. **Interleaving credit assignment**: When computing interleaving scores, join on `interleaving_provenance` from the exposure event — not on variant_id. Each item in the merged list has provenance to its source algorithm.
6. **Guardrail false alarms**: Hourly metrics are noisy. The `consecutive_breaches_required` field exists to reduce false positives. Don't alarm on a single hourly breach unless the config says to.
