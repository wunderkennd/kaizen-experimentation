# Agent-4 — M4a Analysis + M4b Bandit

**Status**: All Phases Complete
**Current Branch**: agent-4/feat/bayesian-ipw-clustering
**Current Milestone**: Bayesian, IPW, clustering, neural bandit
**Blocked By**: —

## Summary

All analysis RPCs wired + PostgreSQL caching. PGO-optimized builds. Bayesian analysis, IPW-adjusted analysis, HC1 clustered SEs, and neural contextual bandit (2-layer MLP behind `gpu` feature flag).

## Key PRs

| PR | Description | Status |
|----|-------------|--------|
| #2 | Welch t-test + SRM + Thompson Sampling | Merged |
| #14 | CUPED variance reduction | Merged |
| #25 | mSPRT + GST sequential testing | Merged |
| #29 | Bootstrap CI + BH-FDR | Merged |
| #38 | Novelty/primacy + interference + interleaving analysis | Merged |
| #54 | LinUCB contextual bandit | Merged |
| #93 | Analysis service scaffold | Merged |
| #107 | All 5 RPCs wired + PG caching (36 tests) | Merged |
| #125 | GST recursive numerical integration | Merged |
| #133 | PGO-optimized builds for M4a/M4b | Merged |
| #136 | GST scipy boundary validation | Merged |
| #140 | Bootstrap BCa/percentile coverage | Merged |
| #145 | CATE wired into RunAnalysis | Merged |
| #147 | M4b load test (k6 gRPC 10K rps) | Merged |
| #151 | M4a-M6 contract tests (12 wire-format tests) | Merged |
| #156 | Bayesian, IPW, clustered SE, neural bandit | Merged |
| #159 | Migrate neural bandit from tch-rs to Candle | Merged |

## Pair Integrations

- Agent-4 <-> Agent-1 (bandit delegation: assignment -> SelectArm)
- Agent-4 <-> Agent-3 (metric summaries -> analysis)
- Agent-4 <-> Agent-6 (analysis results -> UI rendering)

## Test Coverage

- 4 Bayesian golden files
- 3 IPW golden files
- 3 clustering golden files
- 12 M4a-M6 wire-format contract tests
