# Test Coverage Improvement Plan

**Status**: NOT STARTED
**Owners**: All 7 agents (cross-cutting); coordinated by the supervisor.
**Source**: Coverage audit on branch `claude/analyze-test-coverage-YHvlq` (2026-04-25).
**Sprint length**: 5 sprints (TC.0 → TC.4), ~2 weeks each. TC.0 must complete before TC.1–TC.4 begin in parallel.

This plan turns the audit findings into a concrete, agent-assignable backlog. It is written so a Multiclaude / Gas Town worker can pick up any task, read this single section, and ship a PR without needing further coordination.

---

## Audit Findings Summary

| Layer | Production LOC | Tests | Headline gap |
| --- | --- | --- | --- |
| Rust stats (M4a) | ~16K | 359 unit, 86 golden JSON, 19 proptest | 7 modules without golden fixtures (incl. AVLM, switchback, synthetic_control); 9 without proptest |
| Rust bandit (M4b core) | ~5.8K | 130 unit, 5 proptest | OK — well covered |
| Rust policy (M4b binary) | ~3.4K | **18 unit, 0 integration** | `core.rs` is 1,819 LOC with 1 test; `grpc.rs` 599 LOC, 0 tests; **no `tests/` dir** |
| Rust flags (M7) | ~2.4K | **0 unit in src/**, 1 contract, 1 chaos | All 9 source files untested at unit level |
| Rust assignment (M1) | ~3.3K | 50 unit, 9 integration | `service.rs` 864 LOC, 0 tests |
| Rust management (M5 lib) | ~5.0K | 50 unit, 5 proptest | `grpc.rs` 1,365 LOC, `store.rs` 624 LOC, 0 tests each |
| Rust pipeline (M2) | ~2.7K | 44 unit, 22 `#[ignore]` | All Kafka roundtrips dark in CI |
| Go services | ~12K | 568 unit, 92 contract | Healthy — only `orchestration` is thin |
| TypeScript UI | ~21K | 50 component tests | No E2E (no Playwright) |
| SDKs | small | 1–4 tests each | Web/server-go SDKs barely tested; no hash-parity check |
| Tooling | — | — | **No coverage reporting** anywhere (no tarpaulin/llvm-cov/codecov.yaml) |

Cross-module: 9 of the 10 advertised pair contract suites identified. **Missing pairs**: M1↔M2, M2↔M4a, M5↔M7, M7↔M1, M4b↔M5.

CLAUDE.md rule violations:
1. *"Every public function in experimentation-stats gets proptest invariants"* — violated by `bayesian`, `clustering`, `cuped`, `ipw`, `sequential`, `srm`, `ttest`.
2. *"Every new statistical method requires golden files validated against reference R/Python packages to 4+ decimal places"* — violated by `avlm` (ADR-015 P0!), `switchback` (ADR-022), `synthetic_control` (ADR-023), `adaptive_n` (ADR-020), `portfolio` (ADR-019), `multiple_comparison`, `feedback_loop`, plus modules whose `*_golden.rs` test file exists but loads no JSON fixtures.

---

## Sprint Overview

| Sprint | Theme | Blockers | Parallelism |
| --- | --- | --- | --- |
| **TC.0** | Foundations: coverage tooling, baselines, Kafka roundtrip resurrection, weekly Jules cron | — | Sequential — single agent (Agent-2 + Agent-3 helper) |
| **TC.1** | P0 statistical golden fixtures + proptest backfill | TC.0 | Single-agent (Agent-4) but task-parallel |
| **TC.2** | Service-binary unit tests (policy core, flags, management gRPC, assignment service) | TC.0 | Parallel across Agents 1, 4, 5, 7 |
| **TC.3** | Cross-module contract backfill (5 missing pair suites + SDK hash parity) | TC.0; some tasks need TC.2 | Parallel pairs |
| **TC.4** | UI E2E + SQL migration tests + ongoing hygiene | TC.0 | Parallel across Agents 5, 6 |

Total: **31 tasks**, 8 weeks calendar time at ~3 agent-weeks of effort per sprint slot.

---

## Task Spec Schema

Every task below uses this schema. Treat each entry as a self-contained Issue spec — copy the fenced block into `gh issue create`.

```
Task ID:        TC-NNN
Title:          <imperative summary>
Owner:          Agent-N (M-name)
Priority:       P0 | P1 | P2
Sprint:         TC.0 | TC.1 | TC.2 | TC.3 | TC.4
Branch:         agent-N/test/tc-NNN-slug
Estimate:       S (≤1 day) | M (1–3 days) | L (3–5 days)
Depends on:     <list of TC-IDs, or "none">
Unblocks:       <list of TC-IDs, or "none">
Files:          <create vs. modify, exact paths>
Acceptance:     <objectively verifiable bullets>
Verify with:    <exact shell commands>
References:     <ADR / CLAUDE.md / file:line citations>
```

---

## Sprint TC.0 — Foundations

These five tasks gate everything else. Run sequentially, single owner, ≤2 weeks. Without TC.0 we cannot measure progress on TC.1–TC.4.

### TC-001 — Wire `cargo-llvm-cov` into the Rust CI job

```
Owner:        Agent-2 (Pipeline; cross-cutting CI ownership)
Priority:     P0
Sprint:       TC.0
Branch:       agent-2/test/tc-001-llvm-cov
Estimate:     M
Depends on:   none
Unblocks:     TC-002, TC-005

Files:
  CREATE   .github/workflows/coverage.yml
  CREATE   tarpaulin.toml                   # or llvm-cov.toml — see Acceptance
  MODIFY   .github/workflows/ci.yml         # add coverage upload step on PR
  MODIFY   justfile                         # `just coverage` target
  CREATE   docs/guides/coverage.md          # how to read reports

Acceptance:
  - `just coverage` produces `target/llvm-cov/html/index.html` locally.
  - CI uploads `lcov.info` as an artifact on every PR (no failures yet, just measurement).
  - Workspace coverage report excludes generated proto code (`crates/experimentation-proto/src/generated/**`)
    and binary `main.rs` files.
  - No new dependencies in `Cargo.toml` (use `cargo-llvm-cov` as a CI-installed tool).
  - Job timeout ≤25 min on the `rust` runner.

Verify with:
  cargo install cargo-llvm-cov --locked
  cargo llvm-cov --workspace --lcov --output-path lcov.info
  cargo llvm-cov --workspace --html
  test -f target/llvm-cov/html/index.html

References:
  CLAUDE.md → "Testing Commands"
  .github/workflows/ci.yml:80 (rust job structure)
```

### TC-002 — Wire `go test -coverprofile` and Vitest coverage into CI

```
Owner:        Agent-2
Priority:     P0
Sprint:       TC.0
Branch:       agent-2/test/tc-002-go-ts-coverage
Estimate:     S
Depends on:   none (can run parallel with TC-001)
Unblocks:     TC-005

Files:
  MODIFY   .github/workflows/ci.yml           # add `-coverprofile` + vitest --coverage
  MODIFY   ui/vitest.config.ts                # enable coverage.provider = 'v8'
  MODIFY   services/management/justfile or root justfile

Acceptance:
  - Go: `go test -race -coverprofile=cover.out ./...` runs in `services/` and `infra/`,
    uploads `services-cover.out` and `infra-cover.out` artifacts.
  - TS: `vitest run --coverage` runs in `ui/`, uploads `ui/coverage/lcov.info`.
  - Three artifacts visible on every PR: `rust-coverage`, `go-coverage`, `ui-coverage`.
  - No threshold gating yet (measurement-only).

Verify with:
  cd services && go test -race -coverprofile=cover.out ./...
  cd ui && npm run test -- --coverage --run

References:
  ui/vitest.config.ts (jsdom config)
```

### TC-003 — Establish coverage baseline + Codecov integration

```
Owner:        Agent-2
Priority:     P1
Sprint:       TC.0
Branch:       agent-2/test/tc-003-codecov
Estimate:     S
Depends on:   TC-001, TC-002
Unblocks:     all PR-gating coverage thresholds

Files:
  CREATE   codecov.yaml                       # repo root
  CREATE   docs/coordination/status/coverage-baseline.md   # snapshot
  MODIFY   .github/workflows/ci.yml           # add codecov-action upload step

Acceptance:
  - Codecov receives 3 flag-tagged reports per PR (`rust`, `go`, `ui`).
  - `codecov.yaml` declares per-flag baselines (no thresholds enforced yet).
  - Baseline doc records line-coverage % per crate / per Go package, snapshotted on the merge commit
    that lands this PR. Format: markdown table sorted by descending coverage.
  - Coverage badge added to README.md.

Verify with:
  curl -s https://codecov.io/api/gh/wunderkennd/kaizen-experimentation | jq '.commit.totals'

References:
  TC-001, TC-002 outputs
```

### TC-004 — Resurrect ignored Kafka roundtrip tests in nightly CI

```
Owner:        Agent-2
Priority:     P0
Sprint:       TC.0
Branch:       agent-2/test/tc-004-nightly-integration
Estimate:     M
Depends on:   none
Unblocks:     TC-301, TC-302

Files:
  CREATE   .github/workflows/nightly-integration.yml
  MODIFY   docker-compose.test.yml            # ensure Kafka + Zookeeper + Postgres up
  MODIFY   scripts/init-test-db.sh            # idempotent seed for ignored-test fixtures

Context:
  22 #[ignore] tests in pipeline never run in CI:
  - crates/experimentation-pipeline/tests/m2_m3_event_contract.rs:954,985,1027,1058,1079,1112,1138,1169,1391
  - crates/experimentation-pipeline/tests/m3_m5_guardrail_contract.rs:393,428,449
  - crates/experimentation-pipeline/tests/reward_consumer_integration.rs:430,464,503,530,563,596,649

Acceptance:
  - New workflow runs on `cron: "0 4 * * *"` (4 AM UTC, after nightly.yml).
  - Brings up `docker compose -f docker-compose.test.yml up -d --wait` then runs
    `cargo test --workspace -- --include-ignored --test-threads=1`.
  - Failure posts to the `nightly-failures` Slack/issue channel via `gh issue create`.
  - Test runtime budget: ≤30 min total. Tear down compose on exit.

Verify with:
  docker compose -f docker-compose.test.yml up -d --wait
  cargo test -p experimentation-pipeline -- --include-ignored
  docker compose -f docker-compose.test.yml down

References:
  CLAUDE.md → Testing Commands
  .github/workflows/nightly.yml (template)
```

### TC-005 — Auto-schedule the Jules test-coverage workflow

```
Owner:        Agent-2
Priority:     P2
Sprint:       TC.0
Branch:       agent-2/test/tc-005-jules-cron
Estimate:     S
Depends on:   TC-003 (need coverage data to pick lowest-covered crate)
Unblocks:     none (continuous improvement)

Files:
  MODIFY   .github/workflows/jules-test-coverage.yml
  CREATE   scripts/lowest-coverage-crate.sh   # reads codecov API, emits crate name

Acceptance:
  - Workflow gains `schedule: cron: "0 6 * * 1"` (Monday 6 AM UTC).
  - The cron job calls `scripts/lowest-coverage-crate.sh` to pick `inputs.crate`,
    overriding the `workflow_dispatch` requirement.
  - Manual `workflow_dispatch` still works.
  - Jules-generated PRs land with `agent-jules` label.

Verify with:
  bash scripts/lowest-coverage-crate.sh
  # expected: emits one of: experimentation-flags, experimentation-policy, ...

References:
  .github/workflows/jules-test-coverage.yml (current)
```

---

## Sprint TC.1 — P0 Statistical Golden Files & Proptest Backfill

All Agent-4 owned. Tasks may run task-parallel (one branch each), then merge sequentially.

### TC-101 — AVLM golden fixtures (ADR-015) — **HIGHEST PRIORITY**

```
Owner:        Agent-4
Priority:     P0
Sprint:       TC.1
Branch:       agent-4/test/tc-101-avlm-golden
Estimate:     L
Depends on:   TC-001
Unblocks:     none (P0 quality gate for ADR-015)

Files:
  CREATE   crates/experimentation-stats/tests/avlm_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/avlm_no_covariate.json
  CREATE   crates/experimentation-stats/tests/golden/avlm_partial_correlation.json
  CREATE   crates/experimentation-stats/tests/golden/avlm_perfect_covariate.json
  CREATE   crates/experimentation-stats/tests/golden/avlm_large_effect.json
  CREATE   crates/experimentation-stats/tests/golden/avlm_no_effect.json
  CREATE   crates/experimentation-stats/tests/golden/avlm_negative_correlation.json
  CREATE   crates/experimentation-stats/tests/golden/avlm_uneven_arrival.json
  CREATE   scripts/generate_avlm_golden.R
  CREATE   docs/guides/golden-files.md       # reference for future golden authoring

Context:
  experimentation-stats/src/avlm.rs is 1,175 LOC implementing AvlmSequentialTest
  (ADR-015, the #1 ROI item). Currently 15 unit tests but ZERO golden files
  validated against R `avlm`. CLAUDE.md mandates 4-decimal-place precision.

Acceptance:
  - 7 golden JSON fixtures generated from R `avlm` package on a deterministic seed.
  - Each fixture covers a documented scenario:
    * no_covariate (X = []), partial_correlation (ρ ≈ 0.3), perfect_covariate (ρ ≈ 0.99),
    * large_effect (δ = 1.0σ), no_effect (δ = 0), negative_correlation (ρ = -0.4),
    * uneven_arrival (asymmetric n_t/n_c).
  - Each fixture includes: input config, n_treatment, n_control, sufficient stats trace,
    expected confidence sequence bounds at 5 lookpoints, R version + package version.
  - Test asserts CS bounds match R to 4 decimal places using
    experimentation_core::approx_eq_dp(actual, expected, 4).
  - `scripts/generate_avlm_golden.R` is reproducible: `Rscript scripts/generate_avlm_golden.R`
    regenerates all fixtures byte-for-byte.

Verify with:
  cargo test -p experimentation-stats --test avlm_golden
  Rscript scripts/generate_avlm_golden.R   # idempotent

References:
  docs/adrs/015-anytime-valid-regression-adjustment.md
  CLAUDE.md → Golden-File Validation Targets table
  crates/experimentation-stats/src/avlm.rs:1
```

### TC-102 — Switchback golden fixtures (ADR-022)

```
Owner:        Agent-4
Priority:     P0
Sprint:       TC.1
Branch:       agent-4/test/tc-102-switchback-golden
Estimate:     L
Depends on:   TC-001
Unblocks:     none

Files:
  CREATE   crates/experimentation-stats/tests/switchback_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/switchback_homogeneous.json
  CREATE   crates/experimentation-stats/tests/golden/switchback_heterogeneous.json
  CREATE   crates/experimentation-stats/tests/golden/switchback_carryover.json
  CREATE   crates/experimentation-stats/tests/golden/switchback_short_panels.json
  CREATE   scripts/generate_switchback_golden.py     # use DoorDash sandwich estimator reference

Acceptance:
  - 4 golden fixtures matching DoorDash sandwich SE estimator to 4 decimal places.
  - Coverage: HAC (Newey-West) bandwidth selection, randomization inference p-value,
    cluster-robust SE under different panel lengths.
  - Each fixture: panel design (T × N), assignment vector, outcomes, expected ATE +
    SE + p-value.
  - Reproduction script uses pinned `numpy`/`scipy` versions in
    `scripts/requirements-golden.txt`.

Verify with:
  python scripts/generate_switchback_golden.py
  cargo test -p experimentation-stats --test switchback_golden

References:
  docs/adrs/022-switchback-experiment-designs.md
  crates/experimentation-stats/src/switchback.rs:1
```

### TC-103 — Synthetic control golden fixtures (ADR-023)

```
Owner:        Agent-4
Priority:     P1
Sprint:       TC.1
Branch:       agent-4/test/tc-103-synthetic-control-golden
Estimate:     L
Depends on:   TC-001
Unblocks:     none

Context:
  tests/synthetic_control_golden.rs already exists but loads ZERO JSON fixtures.
  Need to wire it up to actual reference data.

Files:
  MODIFY   crates/experimentation-stats/tests/synthetic_control_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/scm_classic_2_donors.json
  CREATE   crates/experimentation-stats/tests/golden/scm_augmented_5_donors.json
  CREATE   crates/experimentation-stats/tests/golden/scm_did_panel.json
  CREATE   crates/experimentation-stats/tests/golden/scm_causal_impact.json
  CREATE   scripts/generate_scm_golden.R     # uses augsynth + CausalImpact

Acceptance:
  - 4 fixtures covering classic SCM, augmented SCM, synthetic DiD, CausalImpact.
  - Match `augsynth::single_augsynth()` results to 4 decimal places.
  - Placebo inference test: 1 fixture with ≥10 placebo permutations, exact p-value.

Verify with:
  Rscript scripts/generate_scm_golden.R
  cargo test -p experimentation-stats --test synthetic_control_golden

References:
  docs/adrs/023-synthetic-control-methods.md
  crates/experimentation-stats/src/synthetic_control.rs:1
```

### TC-104 — Adaptive sample size golden + tests (ADR-020)

```
Owner:        Agent-4
Priority:     P1
Sprint:       TC.1
Branch:       agent-4/test/tc-104-adaptive-n-golden
Estimate:     M
Depends on:   TC-001
Unblocks:     TC-305 (M4b↔M5 adaptive trigger contract)

Files:
  CREATE   crates/experimentation-stats/tests/adaptive_n_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/adaptive_n_promising_zone.json
  CREATE   crates/experimentation-stats/tests/golden/adaptive_n_unfavorable_zone.json
  CREATE   crates/experimentation-stats/tests/golden/adaptive_n_blinded_variance.json
  CREATE   scripts/generate_adaptive_n_golden.R   # uses rpact::getDesignGroupSequential

Acceptance:
  - 3 fixtures cover Mehta/Pocock SiM 2011 zone classifications.
  - Match `rpact` package conditional power calculation to 6 decimal places.
  - Blinded pooled variance test: agree with `pwr::pwr.t2n.test` recalculated n.

Verify with:
  cargo test -p experimentation-stats --test adaptive_n_golden

References:
  docs/adrs/020-adaptive-sample-size-recalculation.md
  crates/experimentation-stats/src/adaptive_n.rs:1
```

### TC-105 — Portfolio optimization golden (ADR-019)

```
Owner:        Agent-4
Priority:     P2
Sprint:       TC.1
Branch:       agent-4/test/tc-105-portfolio-golden
Estimate:     M
Depends on:   TC-001
Unblocks:     none

Files:
  CREATE   crates/experimentation-stats/tests/portfolio_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/portfolio_kelly_sizing.json
  CREATE   crates/experimentation-stats/tests/golden/portfolio_diversified.json
  CREATE   crates/experimentation-stats/tests/golden/portfolio_constrained.json
  CREATE   scripts/generate_portfolio_golden.py

Acceptance:
  - Kelly criterion allocation matches reference Python implementation to 6 dp.
  - 3 fixtures: standard Kelly, fractional-Kelly with risk constraint,
    diversification across 5 experiments.

References:
  docs/adrs/019-portfolio-experiment-optimization.md
  crates/experimentation-stats/src/portfolio.rs:1
```

### TC-106 — Multiple comparison correction golden

```
Owner:        Agent-4
Priority:     P2
Sprint:       TC.1
Branch:       agent-4/test/tc-106-mcc-extended-golden
Estimate:     S
Depends on:   TC-001
Unblocks:     none

Context:
  mcc_golden.rs exists in tests/ but lacks fixtures for new modes
  (Bonferroni-Holm, Benjamini-Hochberg).

Files:
  MODIFY   crates/experimentation-stats/tests/mcc_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/mcc_bonferroni.json
  CREATE   crates/experimentation-stats/tests/golden/mcc_holm.json
  CREATE   crates/experimentation-stats/tests/golden/mcc_bh.json

Acceptance:
  - Match R `p.adjust()` for methods "bonferroni", "holm", "BH" to 6 dp.

Verify with:
  cargo test -p experimentation-stats --test mcc_golden

References:
  crates/experimentation-stats/src/multiple_comparison.rs:1
```

### TC-107 — Sequential mSPRT golden fixtures

```
Owner:        Agent-4
Priority:     P1
Sprint:       TC.1
Branch:       agent-4/test/tc-107-sequential-golden
Estimate:     M
Depends on:   TC-001
Unblocks:     none

Context:
  sequential_golden.rs exists but loads ZERO fixtures.

Files:
  MODIFY   crates/experimentation-stats/tests/sequential_golden.rs
  CREATE   crates/experimentation-stats/tests/golden/msprt_normal_h1.json
  CREATE   crates/experimentation-stats/tests/golden/msprt_normal_h0.json
  CREATE   crates/experimentation-stats/tests/golden/msprt_truncation.json

Acceptance:
  - Match `confseq` Python package mSPRT bounds to 6 dp.
  - Truncation behavior validated against published Howard et al. 2021 examples.

Verify with:
  cargo test -p experimentation-stats --test sequential_golden

References:
  crates/experimentation-stats/src/sequential.rs:1
```

### TC-108 — Backfill proptest blocks for stats modules

```
Owner:        Agent-4
Priority:     P1
Sprint:       TC.1
Branch:       agent-4/test/tc-108-stats-proptest
Estimate:     M
Depends on:   none
Unblocks:     enforces CLAUDE.md proptest rule

Files:
  MODIFY   crates/experimentation-stats/src/bayesian.rs       # +1 proptest block
  MODIFY   crates/experimentation-stats/src/clustering.rs     # +1
  MODIFY   crates/experimentation-stats/src/cuped.rs          # +1
  MODIFY   crates/experimentation-stats/src/ipw.rs            # +1
  MODIFY   crates/experimentation-stats/src/sequential.rs     # +1
  MODIFY   crates/experimentation-stats/src/srm.rs            # +1
  MODIFY   crates/experimentation-stats/src/ttest.rs          # +1

Invariants to assert (per module):
  - bayesian:    posterior mass ∈ [0, 1]; symmetric prior → symmetric posterior under null
  - clustering:  ICC ∈ [0, 1]; design effect ≥ 1 for non-negative ICC
  - cuped:       variance(adjusted) ≤ variance(raw) when |ρ| ≤ 1
  - ipw:         finite weights produce finite estimates; estimator bounded by [min(y), max(y)]
  - sequential:  CI never collapses to 0 width; coverage ≥ 1−α at every lookpoint
  - srm:         χ² statistic ≥ 0; p-value ∈ [0, 1]
  - ttest:       p-value monotone in |t|; symmetric around 0 for two-sided

Acceptance:
  - 7 modules now contain a `proptest!{}` block.
  - Default proptest cases (256 in CI) pass; nightly 10K cases pass.
  - Each invariant uses `assert_finite!()` from experimentation-core where applicable.

Verify with:
  cargo test -p experimentation-stats
  PROPTEST_CASES=10000 cargo test -p experimentation-stats --features simd

References:
  CLAUDE.md → "Proptest invariants: Every public function in experimentation-stats..."
```

---

## Sprint TC.2 — Service-Binary Unit Test Backfill

Parallel across Agents 1, 4, 5, 7. Highest-leverage cleanup of the audit.

### TC-201 — LMAX policy core unit tests + integration suite (M4b)

```
Owner:        Agent-4
Priority:     P0
Sprint:       TC.2
Branch:       agent-4/test/tc-201-policy-core
Estimate:     L
Depends on:   TC-001
Unblocks:     TC-305 (M4b↔M5 contract)

Context:
  crates/experimentation-policy/src/core.rs is 1,819 LOC with 1 #[test].
  This is the ADR-002 LMAX single-thread bandit kernel. It carries the
  highest blast radius of any untested file in the workspace.

Files:
  MODIFY   crates/experimentation-policy/src/core.rs       # +20 unit tests
  MODIFY   crates/experimentation-policy/src/grpc.rs       # +12 unit tests
  MODIFY   crates/experimentation-policy/src/types.rs      # +6 unit tests
  CREATE   crates/experimentation-policy/tests/disruptor_invariants.rs
  CREATE   crates/experimentation-policy/tests/snapshot_roundtrip.rs
  CREATE   crates/experimentation-policy/tests/grpc_handler_matrix.rs
  CREATE   crates/experimentation-policy/benches/single_thread_throughput.rs

Coverage targets (line-coverage in llvm-cov):
  - core.rs:        ≥ 75%
  - grpc.rs:        ≥ 70%
  - snapshot.rs:    ≥ 80%   (currently 4 tests)
  - kafka.rs:       ≥ 80%   (currently 13 tests)

Acceptance:
  - Disruptor invariant proptest: under N=10K random reward events, post-state matches
    sequential reference implementation byte-for-byte.
  - Snapshot roundtrip: serialize → deserialize → re-evaluate yields identical
    arm-selection probabilities.
  - gRPC handler matrix covers all error variants enumerated in
    experimentation-policy/src/grpc.rs response branches.
  - Bench produces a baseline number for single-thread arm selection (target <5μs/op).
  - llvm-cov shows policy crate ≥ 70% overall.

Verify with:
  cargo test -p experimentation-policy
  cargo bench -p experimentation-policy
  cargo llvm-cov -p experimentation-policy --lcov --output-path policy.lcov

References:
  docs/adrs/002-lmax-bandit-core.md
  CLAUDE.md → "Module 4b Bandit"
  crates/experimentation-policy/src/core.rs:1
```

### TC-202 — experimentation-flags unit suite (M7)

```
Owner:        Agent-7
Priority:     P0
Sprint:       TC.2
Branch:       agent-7/test/tc-202-flags-unit
Estimate:     L
Depends on:   TC-001
Unblocks:     TC-303 (M5↔M7), TC-304 (M7↔M1)

Context:
  9 source files, 0 #[test] in src/. The M7 Rust port (ADR-024) cannot ship safely
  without a unit suite.

Files:
  MODIFY   crates/experimentation-flags/src/admin.rs        # +5
  MODIFY   crates/experimentation-flags/src/audit.rs        # +6
  MODIFY   crates/experimentation-flags/src/grpc.rs         # +14
  MODIFY   crates/experimentation-flags/src/kafka.rs        # +5
  MODIFY   crates/experimentation-flags/src/reconciler.rs   # +8
  MODIFY   crates/experimentation-flags/src/store.rs        # +12
  MODIFY   crates/experimentation-flags/tests/contract_test.rs   # resolve TODO at line 346

Coverage targets:
  - reconciler.rs:  ≥ 85%   (convergence safety critical)
  - audit.rs:       ≥ 90%   (chain integrity)
  - store.rs:       ≥ 75%
  - grpc.rs:        ≥ 70%

Acceptance:
  - Reconciler convergence proptest: under N=1K random Kafka event sequences,
    final flag state is deterministic w.r.t. event ordering by timestamp.
  - Audit trail proptest: every state transition produces exactly one audit row;
    chain hash is verifiable.
  - The TODO at tests/contract_test.rs:346 ("Phase 4: implement full response comparison
    using reqwest") is resolved — wire-format parity with Go service to byte-level.
  - llvm-cov shows flags crate ≥ 75% overall.

Verify with:
  cargo test -p experimentation-flags
  cargo llvm-cov -p experimentation-flags

References:
  docs/adrs/024-m7-rust-port.md
  .multiclaude/agents/agent-7-flags.md
```

### TC-203 — experimentation-management/grpc.rs + store.rs unit tests (M5)

```
Owner:        Agent-5
Priority:     P1
Sprint:       TC.2
Branch:       agent-5/test/tc-203-management-grpc
Estimate:     L
Depends on:   TC-001
Unblocks:     none

Files:
  MODIFY   crates/experimentation-management/src/grpc.rs    # +20
  MODIFY   crates/experimentation-management/src/store.rs   # +14

Coverage targets:
  - grpc.rs:   ≥ 65%
  - store.rs:  ≥ 75%

Acceptance:
  - Every RPC handler has at least one happy-path and one error-path unit test
    using a `MockStore` (no real Postgres).
  - Store unit tests use `sqlx::PgPool` from `testcontainers` or an in-memory mock —
    the integration tests already exercise the real DB path.
  - State machine transitions in src/state_machine.rs (already 6 tests) are unchanged.

Verify with:
  cargo test -p experimentation-management

References:
  CLAUDE.md → "Module 5 Management"
  crates/experimentation-management/src/grpc.rs:1
```

### TC-204 — assignment/service.rs + config.rs unit tests (M1)

```
Owner:        Agent-1
Priority:     P1
Sprint:       TC.2
Branch:       agent-1/test/tc-204-assignment-service
Estimate:     M
Depends on:   TC-001
Unblocks:     TC-301 (M1↔M2)

Files:
  MODIFY   crates/experimentation-assignment/src/service.rs   # +12
  MODIFY   crates/experimentation-assignment/src/config.rs    # +6
  MODIFY   crates/experimentation-assignment/src/stream_client.rs   # +4

Coverage targets:
  - service.rs:        ≥ 70%
  - config.rs:         ≥ 80%
  - stream_client.rs:  ≥ 65%

Acceptance:
  - service.rs covers: variant allocation happy path, bandit-arm delegation,
    interleaving roundtrip, error paths for missing config / invalid hash input.
  - Config edge cases: malformed Murmur3 seed, conflicting bucket reuse params,
    invalid traffic fraction.
  - stream_client.rs reconnection backoff covered.

Verify with:
  cargo test -p experimentation-assignment

References:
  CLAUDE.md → "Module 1 Assignment"
  crates/experimentation-assignment/src/service.rs:1
```

### TC-205 — pipeline/kafka.rs unit tests (M2)

```
Owner:        Agent-2
Priority:     P2
Sprint:       TC.2
Branch:       agent-2/test/tc-205-pipeline-kafka
Estimate:     S
Depends on:   TC-001
Unblocks:     none

Files:
  MODIFY   crates/experimentation-pipeline/src/kafka.rs   # +6

Acceptance:
  - Idempotent producer config validation tested.
  - Topic-naming convention assertions tested.
  - Serialization roundtrip via in-process Kafka mock.

Verify with:
  cargo test -p experimentation-pipeline kafka::

References:
  crates/experimentation-pipeline/src/kafka.rs:1
```

---

## Sprint TC.3 — Cross-Module Contract Backfill

Five missing pair contracts + SDK hash parity. Each contract requires the consumer agent to write the test (per CLAUDE.md). Pairs run in parallel.

### TC-301 — M1↔M2 contract: Assignment → Pipeline event emission

```
Owner:        Agent-2 (consumer of assignment events)
Priority:     P1
Sprint:       TC.3
Branch:       agent-2/test/tc-301-m1m2-contract
Estimate:     M
Depends on:   TC-204
Unblocks:     fills the "10 pair suites" claim in CLAUDE.md

Files:
  CREATE   crates/experimentation-pipeline/tests/m1m2_event_contract.rs
  MODIFY   crates/experimentation-pipeline/Cargo.toml         # add experimentation-assignment dev-dep

Acceptance:
  - Wire-format test: M1 emits AssignmentEvent → M2 ingestion validates it without error.
  - Schema invariant: every field in proto AssignmentEvent has a populated test case.
  - Round-trip via in-process Kafka producer/consumer (no network).
  - Edge cases: empty user_id, invalid bucket, malformed timestamp.

Verify with:
  cargo test -p experimentation-pipeline --test m1m2_event_contract

References:
  proto/experimentation/event.proto
  CLAUDE.md → "Contract tests: Cross-module interfaces require wire-format contract tests.
               The consumer agent writes the test."
```

### TC-302 — M2↔M4a contract: Pipeline Delta handoff

```
Owner:        Agent-4 (consumer of pipeline output)
Priority:     P1
Sprint:       TC.3
Branch:       agent-4/test/tc-302-m2m4a-contract
Estimate:     M
Depends on:   TC-004
Unblocks:     none

Files:
  CREATE   crates/experimentation-analysis/tests/m2m4a_delta_contract.rs

Acceptance:
  - Validates Delta Lake table schema written by M2 matches what M4a `delta_reader` expects.
  - Roundtrip: pipeline writes 1K events → analysis reads back → schema matches → counts equal.
  - Uses local filesystem Delta table (no S3).

Verify with:
  cargo test -p experimentation-analysis --test m2m4a_delta_contract

References:
  crates/experimentation-analysis/src/delta_reader.rs:1
  crates/experimentation-pipeline/src/service.rs:1
```

### TC-303 — M5↔M7 contract: Flag-experiment linkage

```
Owner:        Agent-5
Priority:     P1
Sprint:       TC.3
Branch:       agent-5/test/tc-303-m5m7-contract
Estimate:     M
Depends on:   TC-202
Unblocks:     none

Context:
  sql/migrations/004_flag_experiment_linkage.sql exists but is not exercised
  by any test today.

Files:
  CREATE   services/management/internal/handlers/m5m7_contract_test.go
  MODIFY   crates/experimentation-flags/tests/contract_test.rs   # add M5 promotion path

Acceptance:
  - Test PromoteToExperiment on M7 → verifies M5 receives matching ExperimentSpec.
  - Test M5 lifecycle CONCLUDE → verifies M7 reconciler resolves the flag.
  - Wire-format JSON parity to byte level.
  - sql/migrations/004 schema asserted by structural test.

Verify with:
  go test ./services/management/internal/handlers -run M5M7
  cargo test -p experimentation-flags --test contract_test

References:
  sql/migrations/004_flag_experiment_linkage.sql
  proto/experimentation/management.proto
```

### TC-304 — M7↔M1 contract: Flag-driven assignment

```
Owner:        Agent-1
Priority:     P2
Sprint:       TC.3
Branch:       agent-1/test/tc-304-m7m1-contract
Estimate:     M
Depends on:   TC-202, TC-204
Unblocks:     none

Files:
  CREATE   crates/experimentation-assignment/tests/m7m1_flag_contract.rs

Acceptance:
  - When a flag is at 100% rollout, every M1 SelectArm call returns the rollout variant.
  - Percentage rollout (e.g., 30%) yields hash-based deterministic split — verified using
    test-vectors/hash_vectors.json.
  - Multi-variant flag: traffic fractions sum to 100%; no user gets multiple variants.

Verify with:
  cargo test -p experimentation-assignment --test m7m1_flag_contract

References:
  crates/experimentation-flags/src/grpc.rs:1
  crates/experimentation-assignment/src/service.rs:1
```

### TC-305 — M4b↔M5 contract: Auto-pause on guardrail breach

```
Owner:        Agent-5 (consumer of bandit policy state)
Priority:     P1
Sprint:       TC.3
Branch:       agent-5/test/tc-305-m4bm5-contract
Estimate:     M
Depends on:   TC-201
Unblocks:     none

Files:
  CREATE   services/management/internal/handlers/m4bm5_autopause_contract_test.go
  CREATE   crates/experimentation-policy/tests/m4bm5_autopause_contract.rs

Acceptance:
  - When M4b detects guardrail breach (per ADR-008), it emits a PauseRequest
    that M5 honors within one tick of the lifecycle scheduler.
  - Adaptive N trigger from M5 (per TC-104) reaches M4b's `core.rs` and updates
    sample-size budget.
  - Wire-format parity for PauseRequest and AdaptiveNUpdate proto messages.

Verify with:
  go test ./services/management/internal/handlers -run M4bM5Autopause
  cargo test -p experimentation-policy --test m4bm5_autopause_contract

References:
  docs/adrs/008-auto-pause-guardrails.md
  crates/experimentation-policy/src/core.rs:1
```

### TC-306 — SDK hash parity tests across all languages

```
Owner:        Agent-7 (owns hash determinism across SDK boundaries)
Priority:     P1
Sprint:       TC.3
Branch:       agent-7/test/tc-306-sdk-hash-parity
Estimate:     M
Depends on:   none
Unblocks:     none

Context:
  test-vectors/hash_vectors.json contains 10K Murmur3 vectors used by the Rust↔Go↔WASM
  CI parity check (Stage 5). The mobile and server SDKs do not run this validation.

Files:
  CREATE   sdks/web/test/hash-parity.test.ts
  CREATE   sdks/server-go/test/hash_parity_test.go
  CREATE   sdks/server-python/tests/test_hash_parity.py
  CREATE   sdks/ios/Tests/HashParityTests.swift
  CREATE   sdks/android/src/test/java/com/experimentation/HashParityTest.kt
  MODIFY   .github/workflows/mobile-sdk.yml          # add parity step
  MODIFY   scripts/verify_hash_parity.py             # accept --sdk flag for orchestration

Acceptance:
  - Each SDK's test loads test-vectors/hash_vectors.json and asserts its native
    Murmur3 implementation matches the Rust reference for all 10K vectors.
  - CI runs each SDK parity test on its native runner (mobile-sdk.yml).

Verify with:
  cd sdks/web && npm test -- hash-parity
  cd sdks/server-go && go test -run HashParity
  cd sdks/server-python && pytest tests/test_hash_parity.py
  cd sdks/ios && swift test --filter HashParityTests
  cd sdks/android && ./gradlew test --tests "*HashParityTest"

References:
  test-vectors/hash_vectors.json
  scripts/verify_hash_parity.py
  scripts/generate_hash_vectors.py
```

---

## Sprint TC.4 — UI E2E + SQL Migration Tests + Hygiene

### TC-401 — Playwright smoke E2E suite for the experiment wizard

```
Owner:        Agent-6
Priority:     P1
Sprint:       TC.4
Branch:       agent-6/test/tc-401-playwright-e2e
Estimate:     L
Depends on:   none (uses docker-compose.yml)
Unblocks:     none

Files:
  CREATE   ui/playwright.config.ts
  CREATE   ui/e2e/experiment-create.spec.ts
  CREATE   ui/e2e/experiment-results.spec.ts
  CREATE   ui/e2e/portfolio-dashboard.spec.ts
  CREATE   ui/e2e/auto-pause-flow.spec.ts
  CREATE   ui/e2e/audit-log.spec.ts
  CREATE   .github/workflows/ui-e2e.yml          # PR-triggered, docker-compose up
  MODIFY   ui/package.json                        # add @playwright/test

Smoke scenarios:
  1. Create experiment via wizard → assert metric picker → review step → submit → assert listed in /experiments.
  2. Open results dashboard → verify AVLM panel renders → CS plot has data.
  3. Open portfolio dashboard → verify provider-health widget loads.
  4. Trigger guardrail breach via fixture → observe auto-pause UI banner.
  5. Mutate flag → verify audit-log entry appears.

Acceptance:
  - 5 E2E specs run on `docker compose -f docker-compose.yml up -d --wait`.
  - Each spec runs in <30s; total wall time <5 min.
  - Run on Chromium only (skip WebKit/Firefox to limit CI cost).
  - Fixture data loaded via scripts/init-test-db.sh.
  - Failure produces a Playwright trace artifact uploaded to the workflow run.

Verify with:
  cd ui && npx playwright test
  cd ui && npx playwright test --ui   # local debugging

References:
  ui/vitest.config.ts (existing test framework — keep it for unit tests)
  CLAUDE.md → "TypeScript is UI only"
```

### TC-402 — SQL migration round-trip tests

```
Owner:        Agent-5 (M5 owns sqlx migrations) with Agent-7 (M7 migrations)
Priority:     P2
Sprint:       TC.4
Branch:       agent-5/test/tc-402-migration-tests
Estimate:     M
Depends on:   none
Unblocks:     none

Files:
  CREATE   crates/experimentation-management/tests/migrations_roundtrip.rs
  CREATE   crates/experimentation-flags/tests/migrations_roundtrip.rs

Acceptance:
  - Each test ephemerally provisions Postgres (via `testcontainers` crate),
    applies all 13 migrations in sql/migrations/ in order, and asserts:
    * Each table named in the migration exists.
    * Insert + select roundtrip on a representative row (pre-defined fixture per migration).
    * Foreign key constraints from migration 004 (flag↔experiment linkage) hold.
  - Tests gated `#[ignore]` for local laptop runs but UNCONDITIONAL in nightly-integration.yml.

Verify with:
  cargo test -p experimentation-management --test migrations_roundtrip -- --include-ignored
  cargo test -p experimentation-flags --test migrations_roundtrip -- --include-ignored

References:
  sql/migrations/*.sql
  TC-004 (nightly integration workflow)
```

### TC-403 — Resolve in-tree TODO/FIXME contract test stubs

```
Owner:        Agent-7 (flags TODO), Agent-5 (management TODO)
Priority:     P2
Sprint:       TC.4
Branch:       agent-7/test/tc-403-resolve-contract-todos  (split per agent)
Estimate:     S each
Depends on:   TC-202, TC-203
Unblocks:     none

Files:
  MODIFY   crates/experimentation-flags/tests/contract_test.rs:346
  MODIFY   crates/experimentation-management/tests/contract_tests.rs:821

Acceptance:
  - Both TODOs replaced with full proto diff comparison using prost::Message::encode + bytes::Bytes equality.
  - No grep hits for `TODO(Phase 4` in tests/.

Verify with:
  rg 'TODO\(Phase 4' crates/   # expected: no results

References:
  audit finding (2026-04-25)
```

### TC-404 — Add coverage thresholds to PR gate

```
Owner:        Agent-2
Priority:     P1
Sprint:       TC.4
Branch:       agent-2/test/tc-404-coverage-gates
Estimate:     S
Depends on:   TC-003 + measurement period of 4 weeks of TC.0 baselines
Unblocks:     ongoing quality gate

Files:
  MODIFY   codecov.yaml                      # add coverage status checks per flag
  MODIFY   .github/workflows/ci.yml          # codecov action with fail_ci_if_error

Acceptance:
  - PRs cannot regress global coverage by more than 0.5%.
  - Per-crate floors set from baseline:
      experimentation-stats:        baseline + 0%   (already strong)
      experimentation-bandit:        baseline + 0%
      experimentation-management:    baseline + 0%
      experimentation-policy:        baseline (post TC-201) ≥ 70%
      experimentation-flags:         baseline (post TC-202) ≥ 75%
      experimentation-assignment:    baseline (post TC-204) ≥ 70%
  - Override allowed via `codecov: skip` PR label for emergency hotfixes (logged in audit).

Verify with:
  gh pr checks <n>  # see Codecov status

References:
  TC-003 baseline file
  codecov.yaml docs
```

---

## Coordination Protocol

### How to claim a task

1. Read the task spec in this file.
2. Run `gh issue list --label "test-coverage" --state open` to confirm the Issue exists.
   If not, create one using the [Issue Template](#github-issue-templates) below.
3. Comment `Claiming TC-NNN` on the Issue.
4. Assign yourself: `gh issue edit <n> --add-assignee @me`.
5. Create the branch: `git checkout -b <branch-name from spec>`.
6. Update `docs/coordination/status/agent-N-status.md` with current task ID.

### PR conventions

- **Title**: `test(crate): TC-NNN — <imperative summary>`
- **Body** must include:
  - `Closes #<issue-number>`
  - "Acceptance criteria met" checklist with each bullet from the spec checked.
  - llvm-cov / `go cover` / vitest coverage delta for the affected crate.
- **Label**: `test-coverage`, plus `agent-N` ownership label.
- **Reviewer**: 1 reviewer from a different module — contract tests get reviewers from BOTH sides.

### Contract test reviewer rule

For every TC-3xx contract test, the reviewer pool is:

| Contract | Producer reviewer | Consumer reviewer |
| --- | --- | --- |
| TC-301 (M1↔M2) | Agent-1 | Agent-2 (author) |
| TC-302 (M2↔M4a) | Agent-2 | Agent-4 (author) |
| TC-303 (M5↔M7) | Agent-5 (author) + Agent-7 | — |
| TC-304 (M7↔M1) | Agent-7 | Agent-1 (author) |
| TC-305 (M4b↔M5) | Agent-4 | Agent-5 (author) |

Both reviewers must approve before merge.

### Sprint exit criteria

| Sprint | Exit criterion |
| --- | --- |
| TC.0 | All 5 tasks merged; baseline doc landed; nightly-integration.yml green for 3 consecutive nights. |
| TC.1 | All 7 golden tasks (TC-101…TC-107) merged AND TC-108 proptest backfill merged; `cargo test -p experimentation-stats` runs ≥ 380 tests. |
| TC.2 | All 5 service-binary tasks merged; coverage targets met or task re-opened. |
| TC.3 | All 5 contract pairs landed; `rg 'fn test' crates/*/tests/m*_contract*.rs` shows ≥ 50 contract test functions; SDK hash parity green for 5 SDKs. |
| TC.4 | Playwright suite green; migration tests run in nightly-integration; coverage gate enforced on `main`. |

---

## GitHub Issue Templates

### Standard test-coverage Issue

```markdown
## Task ID
TC-NNN

## Owner
Agent-N

## Priority
P0 / P1 / P2

## Sprint
TC.N

## Context
<paste from spec>

## Acceptance Criteria
- [ ] <bullet 1>
- [ ] <bullet 2>
- [ ] Coverage target met (see spec)
- [ ] PR includes `Closes #<this-issue>`

## Files
- CREATE: <path>
- MODIFY: <path>

## Verification
```bash
<paste exact commands>
```

## References
- <ADR / file:line citations>

## Labels
test-coverage, agent-N, P0|P1|P2, sprint-tc-N
```

### Contract test Issue (additional fields)

```markdown
## Producer module
M-X

## Consumer module
M-Y

## Wire format
proto/<file>.proto :: <Message name>

## Pair owners
Producer reviewer: Agent-X
Consumer reviewer (author): Agent-Y
```

---

## Risk Register

| ID | Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- | --- |
| R1 | TC-101 (AVLM golden) blocked by missing R `avlm` package install on the docs runner | Med | High (P0 task) | Pin R version in `scripts/requirements-golden.txt`; pre-install via `actions/setup-r` in coverage.yml |
| R2 | TC-201 (LMAX core proptest) flakes under tokio runtime | Low | High | Use `proptest::test_runner::Config::with_cases(64)` for tokio tests; full 10K only in nightly |
| R3 | TC-401 (Playwright) doubles CI cost per PR | High | Med | Run Playwright only on PRs touching `ui/**` (paths-filter); add nightly full run |
| R4 | TC-202 (flags) reveals byte-level wire-format drift vs Go service blocking ADR-024 cutover | Med | High | Treat as a separate fix Issue; do not block TC-202 merge |
| R5 | TC-004 (nightly integration) flakes due to docker-compose Kafka startup race | Med | Med | Use `--wait` with health checks; bump timeout; auto-retry once before posting failure |
| R6 | TC-306 (SDK parity) requires native iOS/Android runners — slow on free-tier GitHub | Med | Low | Run iOS/Android parity weekly, not per-PR; web/Go/Python parity per-PR |
| R7 | Multiple agents racing on `Cargo.lock` during TC.2 | High | Low | Coordinator merges TC.2 PRs sequentially; rebase-on-merge required |
| R8 | Coverage thresholds (TC-404) cause emergency hotfix friction | Low | Med | `codecov: skip` label as escape hatch; documented in `docs/guides/coverage.md` |

---

## Effort Roll-Up

| Sprint | Tasks | Total estimate (agent-days) | Calendar (parallel) |
| --- | --- | --- | --- |
| TC.0 | 5 | 8 | 1.5 weeks |
| TC.1 | 8 | 22 | 2 weeks (Agent-4 single owner) |
| TC.2 | 5 | 19 | 2 weeks (4 agents in parallel) |
| TC.3 | 6 | 17 | 1.5 weeks (parallel pairs) |
| TC.4 | 4 | 13 | 1.5 weeks |
| **Total** | **31** | **79 agent-days** | **~8 weeks** |

---

## Done Definition

This plan is **complete** when all of the following are true:

1. All 31 task PRs merged to `main`.
2. Codecov dashboard shows: Rust ≥ 75% overall, Go ≥ 80%, TypeScript ≥ 65%.
3. `rg '#\[ignore' crates/` returns ≤ 5 matches (down from 33), each with a justification comment.
4. CLAUDE.md "10 pair integration suites" claim verifiable with one `find` command.
5. No stats module ships without proptest + golden fixture (enforced by a custom clippy lint OR a `scripts/check_stats_coverage.sh` invoked in CI).
6. SDK hash parity green across all 5 client SDKs (web, server-go, server-python, ios, android).
7. PR gate (TC-404) prevents coverage regression on `main`.

---

## Appendix A — File Index by Owner

### Agent-1 (M1 Assignment)
- TC-204: assignment/service.rs, config.rs, stream_client.rs
- TC-304: tests/m7m1_flag_contract.rs

### Agent-2 (M2 Pipeline + cross-cutting CI)
- TC-001 through TC-005: CI tooling
- TC-205: pipeline/kafka.rs
- TC-301: tests/m1m2_event_contract.rs

### Agent-4 (M4a Stats + M4b Bandit)
- TC-101 through TC-108: stats golden + proptest
- TC-201: policy crate (LMAX core)
- TC-302: tests/m2m4a_delta_contract.rs

### Agent-5 (M5 Management)
- TC-203: management/grpc.rs, store.rs
- TC-303: m5m7_contract_test.go
- TC-305: m4bm5_autopause_contract_test.go
- TC-402: management migrations roundtrip
- TC-403 (partial): management contract test TODO

### Agent-6 (M6 UI)
- TC-401: Playwright E2E

### Agent-7 (M7 Flags)
- TC-202: flags crate unit tests
- TC-306: SDK hash parity (cross-cutting)
- TC-403 (partial): flags contract test TODO

---

## Appendix B — Coverage Floor Cheat Sheet

After all sprints complete, these floors are enforced by TC-404:

```yaml
# codecov.yaml excerpt
coverage:
  status:
    project:
      default:
        target: auto
        threshold: 0.5%       # global non-regression
      rust-stats:
        target: 80%
      rust-bandit:
        target: 75%
      rust-policy:
        target: 70%           # set by TC-201
      rust-flags:
        target: 75%           # set by TC-202
      rust-management:
        target: 70%           # set by TC-203
      rust-assignment:
        target: 70%           # set by TC-204
      rust-pipeline:
        target: 65%
      rust-analysis:
        target: 70%
      go-services:
        target: 80%           # already strong
      go-infra:
        target: 65%
      ui:
        target: 65%
```

Floors below set by audit baseline; TC-001/002/003 land them, TC-201/202/203/204 raise them, TC-404 enforces them.

---

**Authored**: 2026-04-25 on branch `claude/analyze-test-coverage-YHvlq`
**Source audit**: see PR description; raw findings inventory at the top of this file
**Maintenance**: update sprint exit criteria as PRs land; this file is the source of truth for test-coverage backlog
