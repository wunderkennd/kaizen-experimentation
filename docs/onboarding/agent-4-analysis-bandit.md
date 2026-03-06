# Agent-4 Quickstart: M4a Statistical Analysis Engine + M4b Bandit Policy Service (Rust)

## Your Identity

| Field | Value |
|-------|-------|
| Modules | M4a: Statistical Analysis Engine, M4b: Bandit Policy Service |
| Language | Rust (all computation) |
| Crates you own | `experimentation-stats` (library), `experimentation-bandit` (library), `experimentation-analysis` (M4a binary), `experimentation-policy` (M4b binary) |
| Proto packages | `experimentation.analysis.v1`, `experimentation.bandit.v1` |
| Infra you own | RocksDB (M4b policy snapshots), PostgreSQL writes (analysis results) |
| Primary SLAs | M4a: full analysis < 60s for 1M-user experiment. M4b: p99 < 15ms SelectArm at 10K rps, crash recovery < 10s. |

## Read These First (in order)

1. **Design doc v5.1** — Sections 7 (M4a), 8 (M4b), 2.1 (crate layering), 2.2 (crash-only), 2.3 (LMAX threading), 2.8 (group sequential tests), 7.3 (analysis methods), 7.4 (core statistical methods)
2. **ADR-002** (LMAX bandit core), **ADR-003** (RocksDB policy state), **ADR-004** (GST alongside mSPRT), **ADR-006** (Cargo workspace)
3. **Proto files** — `analysis_service.proto`, `bandit_service.proto`, `bandit.proto`, `surrogate.proto`, `experiment.proto`
4. **Delta Lake tables** — `delta/delta_lake_tables.sql` (you read: metric_summaries, interleaving_scores, content_consumption, daily_treatment_effects)
5. **Mermaid diagrams** — `lmax_threading.mermaid` (M4b architecture), `crate_graph.mermaid` (your crate dependencies)

## You Own Two Very Different Services

**M4a (Analysis Engine)** is a batch analysis service. It reads from Delta Lake, runs statistical tests, and writes results to PostgreSQL. It's called on-demand (when someone views results) or during CONCLUDING state (final analysis). Latency tolerance: seconds to minutes.

**M4b (Bandit Policy Service)** is a real-time service. It selects arms at 10K rps and updates policy state on every reward. It uses the LMAX single-threaded core pattern. Latency tolerance: milliseconds.

These share the `experimentation-stats` and `experimentation-bandit` algorithm crates but are separate binaries with completely different runtime profiles.

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| M3 (Agent-3) | Delta Lake tables with metric summaries, interleaving scores, etc. | **Yes for M4a** — you have nothing to analyze without M3's output. Use synthetic metric data initially. |
| M2 (Agent-2) | Reward events on Kafka `reward_events` topic | **Yes for M4b** — bandit policy can't learn without rewards. Use synthetic reward stream initially. |
| M1 (Agent-1) | Consistent hash ensures SRM check is meaningful | Not blocking — you validate against hash test vectors. |

## Who Depends on You (downstream)

| Module | What they need from you | Impact if you're late |
|--------|------------------------|----------------------|
| M1 (Agent-1) | M4b `SelectArm` RPC for bandit experiments | M1 falls back to uniform random; degraded but functional. |
| M5 (Agent-5) | Analysis results in PostgreSQL for lifecycle state transitions | M5 can't auto-conclude sequential experiments without your boundary crossing signal. |
| M6 (Agent-6) | All analysis results, novelty analysis, interference analysis, interleaving analysis | **UI has nothing to display without your results.** |

## Your First PR: Welch's t-test + SRM Check

**Goal**: A working `experimentation-stats` crate that computes a two-sample treatment effect with confidence interval, plus an SRM (sample ratio mismatch) chi-squared test.

```
crates/experimentation-stats/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── ttest.rs           # Welch's t-test, effect size, CI
│   ├── srm.rs             # Chi-squared SRM check
│   ├── distribution.rs    # t-distribution CDF/quantile (statrs)
│   └── fail_fast.rs       # NaN/Infinity/overflow detection
└── tests/
    ├── ttest_golden.rs    # Verified against R t.test()
    └── srm_golden.rs      # Verified against R chisq.test()
```

**Acceptance criteria**:
- `welch_ttest(control: &[f64], treatment: &[f64], alpha: f64)` returns `TTestResult { effect, ci_lower, ci_upper, p_value, is_significant, df }`.
- Results match R's `t.test(..., var.equal=FALSE)` to 6 decimal places on 5 golden test datasets.
- `srm_check(observed: &HashMap<String, u64>, expected_fractions: &HashMap<String, f64>)` returns `SrmResult { chi_squared, p_value, is_mismatch }`.
- Any NaN, Infinity, or overflow in intermediate computation triggers a panic with context (fail-fast principle).

**Why this first**: The t-test is the most common analysis method. SRM is run on every experiment. Getting these right, with golden test validation against R, establishes the correctness baseline that every subsequent statistical method builds on.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] `experimentation-stats` crate stub with module structure
- [ ] `experimentation-bandit` crate stub
- [ ] `experimentation-analysis` binary stub (tonic server)
- [ ] `experimentation-policy` binary stub (tonic server + tokio runtime)

### Phase 1 (Weeks 2–7)
**M4a Analysis Engine**:
- [ ] Welch's t-test, z-test for proportions
- [ ] CUPED variance reduction (theta-hat estimator)
- [ ] SRM chi-squared check
- [ ] Delta method for ratio metrics
- [ ] Bootstrap confidence intervals (BCa method)
- [ ] Multiple comparison corrections (Holm-Bonferroni, BH)
- [ ] `RunAnalysis` RPC: read metric_summaries from Delta Lake, compute results, write to PostgreSQL
- [ ] Fail-fast: every arithmetic operation checked for NaN/Infinity/overflow

**M4b Bandit Policy**:
- [ ] Thompson Sampling: Beta posterior (binary rewards), Normal posterior (continuous)
- [ ] LMAX single-threaded core: policy_channel + reward_channel + select! event loop
- [ ] RocksDB snapshot on every reward update
- [ ] Crash recovery: load snapshot, replay from Kafka offset
- [ ] `SelectArm` RPC via tokio gRPC → channel → policy core → oneshot response
- [ ] Warm-up period: uniform random for first N observations per arm

### Phase 2 (Weeks 6–11)
**M4a**:
- [ ] mSPRT (always-valid confidence sequences)
- [ ] GST with O'Brien-Fleming spending function
- [ ] GST with Pocock spending function
- [ ] Bayesian analysis (posterior probability of superiority)
- [ ] IPW-adjusted analysis for bandit experiments
- [ ] Interleaving analysis: sign test, Bradley-Terry model
- [ ] Novelty detection: exponential decay fitting on daily_treatment_effects

**M4b**:
- [ ] LinUCB: ridge regression, Sherman-Morrison rank-1 updates
- [ ] Context feature handling for contextual bandits
- [ ] Min exploration fraction enforcement

### Phase 3 (Weeks 10–17)
**M4a**:
- [ ] Interference analysis: Jensen-Shannon divergence, Jaccard similarity, Gini coefficient
- [ ] Lifecycle segment heterogeneity: Cochran's Q test
- [ ] Clustered standard errors for session-level experiments
- [ ] proptest invariants: p-values ∈ [0,1], CIs contain point estimate, bootstrap CI ⊆ analytical CI

**M4b**:
- [ ] Neural contextual bandit (Candle, 2-layer MLP with dropout — see ADR-011)
- [ ] Cold-start bandit: auto-create experiment, begin exploration, export affinity scores
- [ ] Policy rollback RPC

### Phase 4 (Weeks 16–22)
- [ ] GST boundary validation against R gsDesign (4 decimal places)
- [ ] Bootstrap coverage validation (93–97% on 1000 synthetic datasets)
- [ ] M4b chaos: kill -9 under load, verify recovery < 10 seconds
- [ ] M4b load test: p99 < 15ms at 10K SelectArm rps + 5K reward updates/sec
- [ ] PGO-optimized builds for both M4a and M4b

## Local Development

```bash
# M4a: unit tests (no external deps)
cargo test --package experimentation-stats

# M4a: run analysis server (needs PostgreSQL + Delta Lake / Parquet files)
POSTGRES_DSN=postgres://localhost/experimentation \
DELTA_LAKE_PATH=/tmp/delta \
cargo run --package experimentation-analysis

# M4b: unit tests (no external deps)
cargo test --package experimentation-bandit

# M4b: run policy server (needs Kafka + RocksDB)
KAFKA_BROKERS=localhost:9092 \
ROCKSDB_PATH=/tmp/rocksdb \
cargo run --package experimentation-policy

# Run proptest with extended cases
PROPTEST_CASES=10000 cargo test --package experimentation-stats -- --test-threads=1
```

## Testing Expectations

- **Golden tests**: Every statistical method validated against R (or Python scipy/statsmodels). Store expected values in `tests/golden/` as JSON. Tolerance: 1e-6 for parametric tests, 1e-3 for bootstrap.
- **Property-based (proptest)**: p-values ∈ [0, 1], confidence intervals contain point estimate, symmetric tests produce identical results when groups are swapped, increasing sample size narrows CI.
- **Fail-fast**: Explicitly test edge cases: empty arrays, single observation, all-zero values, extreme outliers (1e308). Verify panic with meaningful message, not silent NaN propagation.
- **M4b determinism**: Same sequence of rewards → same policy state. Test by feeding identical reward streams to two independent policy cores and asserting identical arm selection probabilities.
- **M4b crash recovery**: Write 10,000 rewards, kill -9, restart, verify policy state matches pre-crash state (within Kafka replay tolerance).

## Common Pitfalls

1. **Welch-Satterthwaite degrees of freedom**: The formula involves fourth moments. Numerically unstable for very unequal sample sizes. Use the `statrs` crate's t-distribution, not a hand-rolled approximation.
2. **Bootstrap BCa acceleration**: The jackknife computation for BCa acceleration is O(n²) in naive implementation. Use the leave-one-out optimization to keep it O(n).
3. **mSPRT mixing distribution**: The variance of the mixing distribution controls the tradeoff between power at different effect sizes. Default to a unit-information prior, but make it configurable.
4. **GST alpha spending**: O'Brien-Fleming spending is very conservative early. The first look may require a z-statistic of ~4.3 to reject. This is correct — don't "fix" it.
5. **LMAX channel sizing**: Too small → backpressure causes gRPC timeouts. Too large → memory waste and delayed backpressure signal. Start with 10,000 for policy_channel and 50,000 for reward_channel. Tune based on load test.
6. **RocksDB write amplification**: Default RocksDB config writes ~10x the data you insert. For policy snapshots (small, frequent writes), tune `max_write_buffer_number` and `target_file_size_base` down.
7. **IPW clipping**: Assignment probabilities near 0 cause IPW estimates to explode. Clip probabilities to [min_exploration_fraction, 1.0] before computing IPW weights.
