# Agent-6 — M6 UI (Next.js)

**Status**: All Phases Complete
**Current Branch**: —
**Current Milestone**: All Phase 4 onboarding items complete
**Blocked By**: —

## Summary

Full experiment lifecycle UI with analysis tabs, bandit dashboard, live API integration. Phase 4 complete: performance targets, error resilience, proto-to-UI type alignment, metric browser, wire-format contract tests.

## Key PRs

| PR | Description | Status |
|----|-------------|--------|
| #30 | Experiment list + detail shell (MSW mocked) | Merged |
| #56 | Analysis tabs | Merged |
| #60 | Bandit dashboard | Merged |
| #76 | Surrogate/holdout/guardrail visualizations | Merged |
| #80 | CATE lifecycle segment tab | Merged |
| #81 | QoE/novelty/GST/Lorenz visualizations | Merged |
| #90 | Experiment list search, filter, sort | Merged |
| #108 | Performance targets (code splitting, caching, export worker) | Merged |
| #121 | Layer allocation bucket chart | Merged |
| #130 | Live API integration (37 contract tests) | Merged |
| #137 | Session-level analysis panel | Merged |
| #143 | Error boundary + chaos resilience + M4a wire-format tests | Merged |
| #147 | Proto-to-UI type alignment adapters | Merged |
| #154 | Metric definition browser (/metrics page) | Merged |

## Pair Integrations

- Agent-6 <-> Agent-4 (analysis results -> UI rendering)
- Agent-6 <-> Agent-5 (management API + UI)

## Test Coverage

- 355 total tests
- 27 wire-format contract tests
- 11 metric browser tests
