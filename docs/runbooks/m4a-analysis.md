# M4a Analysis Engine — Operational Runbook

## Service Overview

**Binary**: `experimentation-analysis` (Rust)
**Port**: 50053 (gRPC)
**Profile**: Batch — seconds to minutes latency tolerance
**State**: Stateless — reads Delta Lake, writes PostgreSQL
**Recovery**: Restart binary (SLA: < 2s)

### What it does

Runs statistical tests on experiment data: Welch t-test, SRM check, CUPED, mSPRT, GST, bootstrap, Bayesian, IPW, clustered SE, novelty detection, interference analysis, interleaving analysis.

### RPCs

| RPC | Purpose |
|-----|---------|
| `RunAnalysis` | Compute full analysis for an experiment |
| `GetAnalysisResult` | Read cached results from PostgreSQL |
| `GetInterleavingAnalysis` | Sign test + Bradley-Terry model |
| `GetNoveltyAnalysis` | Exponential decay fitting |
| `GetInterferenceAnalysis` | Content spillover detection |

### Dependencies

| Dependency | Required | Failure mode |
|------------|----------|--------------|
| Delta Lake (Parquet) | Yes | `RunAnalysis` returns Internal error |
| PostgreSQL | Yes | Cache reads fail, falls back to recompute |

---

## Configuration

| Env var | Default | Description |
|---------|---------|-------------|
| `ANALYSIS_GRPC_ADDR` | `[::1]:50053` | gRPC listen address |
| `DELTA_LAKE_PATH` | — | Path to Delta Lake tables (metric_summaries, etc.) |
| `POSTGRES_DSN` | — | PostgreSQL connection string |
| `RUST_LOG` | `info` | Log level (`warn` in production) |

### Startup

```bash
ANALYSIS_GRPC_ADDR="[::1]:50053" \
DELTA_LAKE_PATH=/data/delta \
POSTGRES_DSN="postgres://user:pass@db:5432/experimentation" \
./experimentation-analysis
```

---

## Health Checks

### Quick probe (gRPC)

```bash
# Should return NotFound for nonexistent experiment (service is healthy)
grpcurl -plaintext [::1]:50053 \
  experimentation.analysis.v1.AnalysisService/GetAnalysisResult \
  -d '{"experiment_id":"healthcheck"}'
```

### Prometheus metrics

```
curl -s http://localhost:50053/metrics | grep -E 'analysis_(requests|errors|duration)'
```

---

## Alert Response Procedures

### AnalysisNaNDetected (CRITICAL)

**What**: `assert_finite!()` triggered — a NaN or Infinity was produced during statistical computation.

**Why it matters**: A wrong number silently propagated is worse than a crash. This is the fail-fast safety net.

**Response**:
1. Check logs for the panic message — it includes the variable name and experiment ID
   ```bash
   journalctl -u experimentation-analysis --since "1 hour ago" | grep "FAIL-FAST"
   ```
2. Identify which metric/experiment triggered it
3. Common causes:
   - **Division by zero**: Zero-variance group (all identical values). Check if one variant has no traffic.
   - **Extreme values**: Metric values near `f64::MAX` causing overflow. Check Delta Lake for outlier values.
   - **Empty group**: Zero observations in control or treatment after filtering. Verify metric_summaries has data.
4. The service will have crashed (fail-fast = panic). It restarts automatically. Fix the data issue before re-running analysis.

### AnalysisSRMDetected (CRITICAL)

**What**: Sample Ratio Mismatch — observed group sizes deviate significantly from expected allocation (chi-squared p < 0.001).

**Why it matters**: SRM indicates the randomization unit is broken. All experiment results are unreliable.

**Response**:
1. Do NOT trust any metric results for this experiment
2. Check assignment service (M1) hash consistency:
   ```bash
   grpcurl -plaintext [::1]:50051 \
     experimentation.assignment.v1.AssignmentService/GetAssignment \
     -d '{"experiment_id":"<id>","user_id":"test-user"}'
   ```
3. Check for bot traffic or assignment leakage (users switching variants)
4. Escalate to experiment owner — experiment should be paused via M5

### AnalysisServiceDown (CRITICAL)

**What**: Prometheus cannot reach the service.

**Response**:
1. Check if process is running: `pgrep experimentation-analysis`
2. Check logs: `journalctl -u experimentation-analysis --since "5 min ago"`
3. Restart: `systemctl restart experimentation-analysis`
4. Recovery is instant (stateless, < 2s SLA)

### AnalysisLatencyHigh (WARNING)

**What**: p95 analysis latency exceeds 60 seconds.

**Response**:
1. Check which experiments are being analyzed:
   ```bash
   journalctl -u experimentation-analysis | grep "RunAnalysis" | tail -20
   ```
2. Common causes:
   - **Large experiment**: >1M users with many metrics. Expected — consider batching.
   - **Delta Lake I/O**: Slow Parquet reads. Check disk throughput.
   - **PostgreSQL**: Slow cache writes. Check connection pool and locks.
3. For PGO-optimized builds: `just pgo-build-analysis`

### AnalysisErrorRate (WARNING)

**What**: >5% of requests are failing.

**Response**:
1. Check error types in logs:
   ```bash
   journalctl -u experimentation-analysis | grep "ERROR" | tail -50
   ```
2. Common causes:
   - Delta Lake path missing or corrupted
   - PostgreSQL connection refused
   - Experiment not found (expected for early-stage experiments)

---

## Debugging

### Rerun analysis for a specific experiment

```bash
grpcurl -plaintext [::1]:50053 \
  experimentation.analysis.v1.AnalysisService/RunAnalysis \
  -d '{"experiment_id":"exp-123"}'
```

### Check cached results

```bash
grpcurl -plaintext [::1]:50053 \
  experimentation.analysis.v1.AnalysisService/GetAnalysisResult \
  -d '{"experiment_id":"exp-123"}'
```

### Inspect Delta Lake data

```python
import pyarrow.parquet as pq
table = pq.read_table("/data/delta/metric_summaries/")
print(table.schema)
print(table.filter(pc.field("experiment_id") == "exp-123").to_pandas())
```

### Run golden file validation

```bash
cargo test --package experimentation-stats -- golden
UPDATE_GOLDEN=1 cargo test --package experimentation-stats  # Regenerate after intentional changes
```

---

## Performance Tuning

### Benchmarks

```bash
just bench-crate experimentation-stats
```

Key benchmarks and typical ranges (release mode, Apple M-series):
- `welch_ttest_10k`: ~50-100us
- `cuped_adjustment_10k`: ~200-400us
- `bootstrap_bca_1k_2000r`: ~20-50ms (dominated by resampling)
- `bayesian_beta_binomial_10k`: ~15-30ms (100K Monte Carlo draws)
- `ipw_estimate_10k`: ~100-300us
- `clustered_se_10k_500clusters`: ~500us-2ms

### PGO build

```bash
just pgo-build-analysis  # 3-phase: instrument → profile → optimize
```
