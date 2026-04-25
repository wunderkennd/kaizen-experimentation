# Test Coverage Improvement — Status Tracker

**Plan**: `docs/coordination/test-coverage-improvement-plan.md`
**Bootstrap**: `bash scripts/create-test-coverage-issues.sh wunderkennd/kaizen-experimentation`
**Active milestone**: TC.0 (run `gh issue list --milestone "TC.0: Foundations" --state open` to see)

This file is the human-readable dashboard. The source of truth is GitHub Issues + PR links — update the `Status` and `PR` columns when each task lands.

## Conventions

| Symbol | Meaning |
| --- | --- |
| ⚪ | Not started — Issue exists, no branch yet |
| 🟡 | In progress — branch pushed, draft PR opened |
| 🟠 | Review — non-draft PR, awaiting reviewer(s) |
| 🟢 | Merged |
| ⚠ | Blocked — see `blocked` label on Issue |
| ⛔ | Failed CI / re-opened |

## Sprint Status

| Sprint | Tasks | Done | In Flight | Blocked | Exit criteria |
| --- | --- | --- | --- | --- | --- |
| TC.0 | 5 | 0 | 0 | 0 | All 5 merged; baseline doc landed; nightly-integration green ×3 |
| TC.1 | 8 | 0 | 0 | 0 | 7 golden + 1 proptest task merged; `cargo test -p experimentation-stats` ≥ 380 tests |
| TC.2 | 5 | 0 | 0 | 0 | All 5 service-binary tasks merged; coverage targets met |
| TC.3 | 6 | 0 | 0 | 0 | 5 contract pairs landed; SDK hash parity green ×5 |
| TC.4 | 4 | 0 | 0 | 0 | Playwright green; migration tests in nightly; coverage gate enforced |

## Sprint TC.0 — Foundations

| # | Task | Owner | Status | PR | Notes |
| --- | --- | --- | --- | --- | --- |
| TC-001 | Wire cargo-llvm-cov into Rust CI | Agent-2 | ⚪ | — | — |
| TC-002 | Wire go test -coverprofile + Vitest coverage | Agent-2 | ⚪ | — | — |
| TC-003 | Coverage baseline + Codecov integration | Agent-2 | ⚪ | — | Depends on TC-001, TC-002 |
| TC-004 | Resurrect ignored Kafka roundtrip tests in nightly CI | Agent-2 | ⚪ | — | 22 #[ignore] tests in pipeline |
| TC-005 | Auto-schedule Jules test-coverage workflow weekly | Agent-2 | ⚪ | — | Depends on TC-003 |

## Sprint TC.1 — Statistical Goldens

| # | Task | Owner | Status | PR | Notes |
| --- | --- | --- | --- | --- | --- |
| TC-101 | AVLM golden fixtures (ADR-015) | Agent-4 | ⚪ | — | **HIGHEST PRIORITY** |
| TC-102 | Switchback golden fixtures (ADR-022) | Agent-4 | ⚪ | — | DoorDash sandwich estimator |
| TC-103 | Synthetic control golden fixtures (ADR-023) | Agent-4 | ⚪ | — | augsynth + CausalImpact |
| TC-104 | Adaptive sample size golden + tests (ADR-020) | Agent-4 | ⚪ | — | Unblocks TC-305 |
| TC-105 | Portfolio optimization golden (ADR-019) | Agent-4 | ⚪ | — | — |
| TC-106 | Multiple comparison correction golden | Agent-4 | ⚪ | — | — |
| TC-107 | Sequential mSPRT golden fixtures | Agent-4 | ⚪ | — | — |
| TC-108 | Backfill proptest blocks for stats modules | Agent-4 | ⚪ | — | 7 modules: bayesian/clustering/cuped/ipw/sequential/srm/ttest |

## Sprint TC.2 — Service Binaries

| # | Task | Owner | Status | PR | Notes |
| --- | --- | --- | --- | --- | --- |
| TC-201 | LMAX policy core unit tests + integration suite (M4b) | Agent-4 | ⚪ | — | core.rs is 1,819 LOC w/ 1 test today |
| TC-202 | experimentation-flags unit suite (M7) | Agent-7 | ⚪ | — | 0 unit tests in src/ today |
| TC-203 | management grpc.rs + store.rs unit tests (M5) | Agent-5 | ⚪ | — | 1,365 + 624 LOC, 0 tests |
| TC-204 | assignment service.rs + config.rs unit tests (M1) | Agent-1 | ⚪ | — | Unblocks TC-301 |
| TC-205 | pipeline kafka.rs unit tests (M2) | Agent-2 | ⚪ | — | — |

## Sprint TC.3 — Contract Backfill

| # | Task | Owner | Status | PR | Notes |
| --- | --- | --- | --- | --- | --- |
| TC-301 | M1↔M2 contract: Assignment → Pipeline event emission | Agent-2 | ⚪ | — | Reviewer: Agent-1 + Agent-2 |
| TC-302 | M2↔M4a contract: Pipeline Delta handoff | Agent-4 | ⚪ | — | Reviewer: Agent-2 + Agent-4 |
| TC-303 | M5↔M7 contract: Flag-experiment linkage | Agent-5 | ⚪ | — | Reviewer: Agent-5 + Agent-7 |
| TC-304 | M7↔M1 contract: Flag-driven assignment | Agent-1 | ⚪ | — | Reviewer: Agent-7 + Agent-1 |
| TC-305 | M4b↔M5 contract: Auto-pause on guardrail breach | Agent-5 | ⚪ | — | Reviewer: Agent-4 + Agent-5 |
| TC-306 | SDK hash parity tests across all 5 client SDKs | Agent-7 | ⚪ | — | web/server-go/server-python/ios/android |

## Sprint TC.4 — UI E2E + Hygiene

| # | Task | Owner | Status | PR | Notes |
| --- | --- | --- | --- | --- | --- |
| TC-401 | Playwright smoke E2E suite for the experiment wizard | Agent-6 | ⚪ | — | 5 specs |
| TC-402 | SQL migration round-trip tests | Agent-5 | ⚪ | — | testcontainers Postgres |
| TC-403 | Resolve in-tree TODO/FIXME contract test stubs | Agent-7/Agent-5 | ⚪ | — | 2 TODOs in tests/ |
| TC-404 | Add coverage thresholds to PR gate | Agent-2 | ⚪ | — | After 4-week measurement period |

## Coordinator Update Protocol

When a PR lands:

1. Update the matching row's `Status` (⚪ → 🟡 → 🟠 → 🟢) and paste the PR URL into the `PR` column.
2. Bump the rollup counts in the Sprint Status table at the top.
3. If a task unblocks downstream work (per Depends-On in the spec), comment on those Issues to wake them up.
4. Once all rows in a sprint are 🟢, comment on each unblocked downstream sprint's Issues and consider running `just evening tc.<next>`.

When a worker stalls:

- Per-worker heartbeat files (`agent-N-status.md` in this directory) are gitignored — read them locally with `multiclaude worker status <agent>`.
- If a worker hasn't pushed in >30 min, run `multiclaude worker nudge <agent>` or kill+restart with the original Issue body.

## Quick Commands

```bash
# See open test-coverage Issues for the current sprint
gh issue list --label test-coverage --label sprint-tc-0 --state open

# See all test-coverage PRs
gh pr list --label test-coverage --state all --limit 50

# Re-launch a failed worker by Issue number
gh issue view <num> --json title,body --jq '.title + ". " + .body' | \
  multiclaude worker create "$(cat -)"

# See coverage delta for a PR (after TC-003 lands)
gh pr view <num> --json comments --jq '.comments[].body' | grep -i coverage
```

## Done Definition

The whole plan completes when all of:

1. All 31 issues closed (PRs merged).
2. Codecov dashboard shows: Rust ≥ 75%, Go ≥ 80%, TypeScript ≥ 65%.
3. `rg '#\[ignore' crates/` returns ≤ 5 matches with justification comments.
4. CLAUDE.md "10 pair integration suites" claim verifiable with one `find` command (currently 9; this plan adds 5 more for 14 total).
5. No stats module ships without proptest + golden fixture (enforced by `scripts/check_stats_coverage.sh` invoked in CI per TC-404).
6. SDK hash parity green across all 5 client SDKs.
7. PR gate (TC-404) prevents coverage regression on `main`.
