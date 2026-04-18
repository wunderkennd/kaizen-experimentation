# ADR-025: Conditional Port of M5 Experiment Management Service from Go to Rust

- **Status**: Proposed (conditional — see Decision Trigger below)
- **Date**: 2026-03-20
- **Author**: Agent-5 (Management) / Platform Architecture
- **Supersedes**: Partially supersedes ADR-001 language selection (for M5 only, if triggered)

## Context

M5 is the largest Go service in the platform: 16 RPCs, RBAC interceptor, lifecycle state machine with TOCTOU-safe PostgreSQL transitions, Kafka guardrail consumer, bucket reuse management, and 21+ contract tests. ADR-001 placed M5 in Go because its workload is CRUD orchestration — the classic Go sweet spot.

Phase 5 ADRs change this calculus. Six proposed ADRs add statistical computation or ML-adjacent logic to M5's responsibilities:

| ADR | New M5 Responsibility | Computation Required |
|-----|----------------------|---------------------|
| 015 (AVLM) | MLRATE model training trigger during STARTING | Indirect — M3 trains, M5 orchestrates |
| 018 (E-Values) | `OnlineFdrController` singleton — platform-level FDR state | E-value thresholding (simple math), but state management is the concern |
| 019 (Portfolio) | Optimal alpha recommendation, traffic allocation optimizer, decision rule evaluation | Power analysis, conditional power, annualized impact — all in `experimentation-stats` |
| 020 (Adaptive N) | Interim analysis trigger, conditional power computation, zone classification, experiment extension | Conditional power requires variance estimation, boundary computation, spending function recalculation |
| 021 (Feedback Loops) | ModelRetrainingEvent correlation, bias-corrected effect estimation | Time-series regression, correlation testing |
| 013 (Meta-Experiments) | Validation of MetaVariantConfig, cross-variant business outcome configuration | Configuration validation (lightweight) |

The tension is clear: M5 currently delegates all statistical computation to M4a via gRPC. But ADRs 018, 019, and 020 require M5 to make *management decisions* that depend on statistical computations — "should I extend this experiment?" (020), "what alpha should I recommend?" (019), "has the FDR budget been exceeded?" (018). Each of these requires a synchronous M5→M4a RPC, increasing latency on management operations and coupling M5's availability to M4a's.

There are three ways to resolve this tension:

**Option A: Keep Go, add RPCs.** M5 calls M4a for every statistical computation. This adds RPC latency to management operations and creates a tight coupling between M5 and M4a. The `OnlineFdrController` (ADR-018) is particularly awkward — it's a singleton that must be consulted on every experiment conclusion, but its state (alpha wealth, rejection count) lives in M5's PostgreSQL while its computation (e-value thresholding) requires `experimentation-stats`.

**Option B: Keep Go, add FFI.** M5 calls `experimentation-stats` Rust functions via CGo/FFI, similar to M7's current hash bridge. This eliminates RPC latency but reintroduces the FFI complexity that ADR-024 eliminates for M7 — and at a much wider API surface (power analysis, e-value computation, conditional power, spending functions) than the single hash function M7 needs.

**Option C: Port M5 to Rust.** M5 imports `experimentation-stats` as a workspace crate dependency. Statistical computations are direct function calls — no FFI, no RPC. The `OnlineFdrController` directly uses `experimentation-stats::e_value_grow()`. Power analysis uses `experimentation-stats::power_analysis()`. Conditional power uses `experimentation-stats::conditional_power()`.

This ADR proposes Option C, conditional on the implementation depth of Phase 5.

## Decision

Port M5 from Go to Rust **if and when** the implementation plan commits to three or more of ADRs {015, 018, 019, 020, 021}. Until that trigger is met, M5 remains in Go and uses M4a RPCs for statistical computation (Option A).

### Decision Trigger

**Port M5 to Rust when** the platform commits to implementing ≥ 3 of the following ADRs:
- ADR-018 (E-Values / OnlineFdrController)
- ADR-019 (Portfolio Optimization)
- ADR-020 (Adaptive Sample Size)
- ADR-021 (Feedback Loop Interference)
- ADR-015 Phase 2 (MLRATE — M5 triggers model training during STARTING)

**Rationale for the threshold**: Each of these ADRs adds one M5→M4a RPC dependency for statistical computation. At 1–2 ADRs, the RPC overhead is tolerable (~5ms per call, infrequent). At 3+, the cumulative coupling becomes architecturally significant: M5's management decisions depend on M4a availability for routine operations (experiment creation, conclusion, interim analysis), and the Go→Rust boundary expands from "M5 orchestrates, M4a computes" to "M5 interleaves orchestration with statistical reasoning on every management action."

### Implementation Plan (when triggered)

**Phase 1: Core service scaffold** (~2 weeks)
- Create `crates/experimentation-management/` in the workspace.
- tonic service with tonic-web for JSON HTTP mode (ConnectRPC JSON wire-format compatibility).
- PostgreSQL via sqlx — migrate 5 SQL migration files. Compile-time checked queries.
- RBAC interceptor: tonic interceptor extracting auth context, enforcing 4-level role hierarchy.
- Lifecycle state machine: `UPDATE ... WHERE state = $expected` with `rows_affected() == 1` — identical TOCTOU pattern, now in sqlx.
- CRUD RPCs: CreateExperiment, GetExperiment, ListExperiments, UpdateExperiment, ArchiveExperiment.

**Phase 2: Lifecycle and orchestration** (~2 weeks)
- State transitions: DRAFT→STARTING→RUNNING→CONCLUDING→CONCLUDED→ARCHIVED.
- STARTING validation: all experiment type validators, including new types (META, SWITCHBACK, QUASI).
- Guardrail Kafka consumer: rdkafka (already used by M4b), same consumer group semantics.
- Bucket reuse: allocator with `ErrOverlappingRanges`, cooldown enforcement.
- StreamConfigUpdates: tonic streaming RPC to M1.
- Metric CRUD, Layer management, Targeting rules.

**Phase 3: Phase 5 statistical integration** (~1 week)
- Direct `experimentation-stats` imports — no RPC, no FFI:
  - `OnlineFdrController` uses `e_value_grow()`, `e_value_avlm()`.
  - Portfolio optimizer uses `power_analysis()`, `conditional_power()`.
  - Adaptive sample size uses `conditional_power()`, spending function recalculation.
  - Feedback loop detection delegates heavy computation to M4a but correlates `ModelRetrainingEvent` data locally.
- Portfolio dashboard data: annualized impact aggregation, traffic utilization, experiment throughput — all computed by M5 and served to M6.

**Phase 4: Validation and cutover** (~1 week)
- Port all 11 M5-M6 contract tests (wire-format compatibility).
- Port all 10 M1-M5 contract tests.
- Port RBAC tests, lifecycle state machine tests, guardrail consumer tests.
- Shadow traffic: run Rust M5 alongside Go M5 for 48 hours, compare all RPC responses.
- Cutover: DNS/load balancer switch. Decommission Go M5.

### What Gets Ported

| Component | Go Lines (est.) | Rust Complexity | Notes |
|-----------|-----------------|-----------------|-------|
| CRUD RPCs (6) | ~800 | Low | tonic handlers + sqlx |
| Lifecycle state machine | ~600 | Medium | TOCTOU pattern identical in sqlx |
| RBAC interceptor | ~300 | Low | tonic interceptor + auth context extraction |
| STARTING validators (6 experiment types + 3 new) | ~500 | Medium | Must port all validation logic exactly |
| Guardrail Kafka consumer | ~200 | Low | rdkafka, same pattern as M4b |
| Bucket reuse allocator | ~400 | Medium | Overlap detection, cooldown state |
| StreamConfigUpdates | ~200 | Low | tonic streaming server |
| Metric CRUD | ~300 | Low | sqlx |
| Layer management | ~200 | Low | sqlx |
| Targeting rules | ~200 | Low | Proto predicate tree evaluation |
| MetricType filter | ~50 | Low | Query parameter |
| Audit trail | ~150 | Low | PostgreSQL insert |
| **Total existing** | **~3,900** | | |
| OnlineFdrController (ADR-018) | N/A (new) | Medium | Direct `experimentation-stats` import |
| Portfolio optimizer (ADR-019) | N/A (new) | Medium | Direct `experimentation-stats` import |
| Adaptive N trigger (ADR-020) | N/A (new) | Low | Direct `experimentation-stats` import |
| Feedback loop correlation (ADR-021) | N/A (new) | Medium | Time-series analysis |
| Meta-experiment validation (ADR-013) | N/A (new) | Low | Config validation |
| Switchback config validation (ADR-022) | N/A (new) | Low | Config validation |
| Quasi-experiment management (ADR-023) | N/A (new) | Low | Simplified lifecycle |

Estimated Rust output: ~3,500 lines (existing) + ~1,500 lines (Phase 5 additions) = ~5,000 lines.

### What Stays in Go

| Service | Rationale for Staying |
|---------|----------------------|
| M2-Orch (Event Pipeline orchestration) | Pure I/O orchestration (Kafka admin, alerting). No Rust dependencies. |
| M3 (Metric Computation) | Spark SQL templating and job submission. No Rust dependencies. The Spark/Python ecosystem is Go-orchestrated. |

After M5 and M7 (ADR-024) are ported, the Go surface reduces to M2-Orch and M3 — both purely I/O-bound orchestration services with zero Rust computation dependencies. The `experimentation-ffi` crate is deleted entirely.

### Crate Dependency Graph (post-port)

```
experimentation-core ──────────────────────────────────────────────┐
experimentation-hash ─────┬────────────────────────┐               │
experimentation-proto ────┼──────────┬─────────────┤               │
experimentation-stats ────┼──────────┼─────────┐   │               │
experimentation-bandit ───┼──────────┼─────────┼───┤               │
experimentation-interleaving ─┐      │         │   │               │
                              │      │         │   │               │
                              ▼      ▼         ▼   ▼               ▼
                        assignment analysis  policy flags    management
                          (M1)     (M4a)    (M4b)  (M7)       (M5)
                                                          [NEW: direct
                                                           stats import]
```

Note that `experimentation-management` imports `experimentation-stats` directly — this is the architectural payoff. No FFI, no RPC hop, no separate process for statistical computation in management decisions.

## Consequences

### Positive

- **Direct `experimentation-stats` access** — the primary motivation. M5's management decisions that require statistical computation (power analysis, e-value thresholding, conditional power, spending function recalculation) become direct function calls. Latency: ~1μs (function call) vs. ~5ms (gRPC RPC). Availability: no M4a dependency for management operations.
- **OnlineFdrController is a natural Rust struct** — it maintains state in PostgreSQL and computes e-value thresholds using `experimentation-stats`. In Go, this requires either FFI or an M4a RPC on every experiment conclusion. In Rust, it's `stats::e_value_grow()` in the same process.
- **Uniform language for stateful services** — after this port, all services that maintain application state (M1 config cache, M4b RocksDB, M5 PostgreSQL lifecycle, M7 PostgreSQL flags) are Rust. Go is reserved for stateless I/O orchestration (M2-Orch, M3).
- **Workspace-level refactoring** — when `experimentation-stats` APIs change (e.g., AVLM replaces mSPRT internals per ADR-015), M5's consumers update in the same `cargo build`. No proto regeneration, no version skew.
- **Eliminates the last FFI consumer** — combined with ADR-024, the `experimentation-ffi` crate and all CGo infrastructure are deleted.

### Negative

- **Largest rewrite in the platform's history**: ~3,900 lines of working Go code ported to Rust. 6 weeks of effort for a service that functions correctly today.
- **Loss of Go's goroutine model for M5**: M5's concurrent operations (Kafka consumer + RPC server + StreamConfigUpdates) are elegantly expressed in Go goroutines. The Rust equivalent (tokio tasks + channels) is functionally identical but syntactically heavier. M5's Kafka consumer, in particular, benefits from Go's select-over-channels pattern.
- **sqlx migration**: M5's PostgreSQL interaction is mature (5 migration files, TOCTOU-safe transitions, audit trail). sqlx is equally capable but requires re-validating every query at compile time. Any query that M4a's `AnalysisStore` hasn't already exercised must be newly validated.
- **RBAC interceptor**: The Go ConnectRPC interceptor is ~100 lines. A tonic interceptor is similar in concept but different in API (tower middleware vs. connect-go interceptor). The auth context extraction and role hierarchy enforcement must be re-implemented and tested.
- **Contract test re-validation**: 21+ contract tests (11 M5-M6, 10 M1-M5) must pass against the Rust service with identical wire-format. Any proto3 serialization difference (zero-value handling, int64 string encoding) is a regression.

### Risks

- **Regression in lifecycle state machine**: The TOCTOU-safe state transitions are the most critical correctness property in M5. The Go implementation uses `database/sql` with `ExecContext` and checks `RowsAffected() == 1`. The Rust equivalent uses sqlx with `execute` and checks `rows_affected() == 1`. The semantics are identical, but any subtle difference in transaction isolation or prepared statement caching could introduce a regression. Mitigation: port the Go state machine tests line-by-line; add proptest for concurrent state transition invariants.
- **Kafka consumer group migration**: Switching from Sarama (Go) to rdkafka (Rust) requires careful offset management. If the Rust consumer joins the same consumer group, it must resume from the committed offset without message loss. If it joins a new group, there will be a brief period of duplicate processing. Mitigation: deploy with a new consumer group; idempotent processing ensures duplicates are harmless.
- **tonic-web JSON compatibility**: M6 uses `@connectrpc/connect-web` which expects ConnectRPC JSON wire format. tonic-web's JSON mode must produce byte-identical responses for all 16 RPCs. Mitigation: shadow traffic for 48 hours before cutover.
- **Premature port**: If Phase 5 implementation stalls at 1–2 ADRs (below the trigger threshold), porting M5 would be wasted effort. The decision trigger is designed to prevent this, but organizational priority shifts could strand the work.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected (conditionally) |
|-------------|------|------|------------------------------|
| Keep Go + M4a RPCs (Option A) | No rewrite; service works | Growing RPC coupling; M5 availability depends on M4a for management operations; latency on every statistical management decision | Acceptable at 1–2 ADRs; unacceptable at 3+ |
| Keep Go + FFI bridge (Option B) | No rewrite; lower latency than RPC | Reintroduces FFI complexity at a wider API surface than M7's single hash function; debugging/profiling across language boundary | Moves in the wrong direction — ADR-024 eliminates FFI |
| Port M5 unconditionally now | Simplifies architecture immediately | 6 weeks of effort before Phase 5 confirms which ADRs will be implemented; risk of premature optimization | The trigger threshold exists to prevent this |
| Microservice split: extract statistical M5 functions into a Rust sidecar | Keeps Go M5 for CRUD; Rust sidecar for stats | Two services where one suffices; deployment complexity; state synchronization between Go M5 and Rust sidecar | Over-engineered; the sidecar is just M4a with extra steps |
| Embed Python (PyO3) in Go M5 for statistical computation | Python has mature stats libraries | Three languages in one service; runtime overhead; deployment complexity | Absurd complexity for the problem being solved |

## Decision Criteria Summary

| Condition | Action |
|-----------|--------|
| ≥ 3 of {ADR-015 P2, ADR-018, ADR-019, ADR-020, ADR-021} committed | **Port M5 to Rust** per this ADR |
| 1–2 of the above committed | Keep Go M5; use M4a RPCs for statistical computation |
| 0 of the above committed | Keep Go M5; no changes needed |

## References

- ADR-001 (Language selection — this ADR narrows the Go surface further)
- ADR-010 (ConnectRPC — tonic-web replaces connect-go for M5)
- ADR-024 (M7 Rust port — eliminates FFI crate; this ADR completes the elimination)
- ADR-018 (E-values — OnlineFdrController is the canonical example of M5 needing stats)
- ADR-019 (Portfolio — optimal alpha and traffic allocation require power analysis in M5)
- ADR-020 (Adaptive N — conditional power computation triggered by M5)
- M5 contract tests: 11 M5-M6 + 10 M1-M5 (must pass identically post-port)
