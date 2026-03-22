# Agent-6 — M6 UI (Next.js)

**Status**: All Phases Complete + Polish
**Current Branch**: `agent-6/feat/ipw-results`
**Current Milestone**: IPW integration + experiment wizard + advanced pages
**Blocked By**: —

## Summary

Full experiment lifecycle UI with analysis tabs, bandit dashboard, live API integration. Phase 4 complete: performance targets, error resilience, proto-to-UI type alignment, metric browser, wire-format contract tests. Post-phase polish: experiment creation wizard (5-step type-aware flow), IPW-adjusted results integration, monitoring/comparison/audit pages.

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
| #169 | Experiment creation wizard (5-step type-aware flow) | Merged |
| #176 | Real-time monitoring page (/monitoring) | Open |
| #177 | Experiment comparison view (/compare) | Open |
| #178 | Audit log viewer (/audit) | Open |
| — | IPW-adjusted results integration | In progress |

## Recent Changes (Week 3+)

- **Experiment wizard**: Replaced monolithic form with 5-step wizard (Basics → Type Config → Variants → Metrics → Review). Type-specific config forms for INTERLEAVING, SESSION_LEVEL, MAB/CONTEXTUAL_BANDIT, PLAYBACK_QOE. 37 new tests (26 validation + 11 integration).
- **IPW results**: IpwToggle + IpwDetailsPanel components, TreatmentEffectsTable IPW mode, bandit experiment analysis mock data with IPW. 3 new wire-format contract tests. Renders Agent-4's Hájek-estimated treatment effects for bandit experiments.
- **Monitoring page**: Real-time experiment health dashboard with guardrail alerts, SRM checks, metric summaries.
- **Comparison view**: Side-by-side experiment comparison with aligned metric tables.
- **Audit log viewer**: Searchable audit trail of experiment state transitions.

## Pair Integrations

- Agent-6 <-> Agent-4 (analysis results -> UI rendering, IPW wire-format contract)
- Agent-6 <-> Agent-5 (management API + UI)

## Test Coverage

- 416 total tests (+ 6 skipped)
- 40 wire-format contract tests (37 original + 3 IPW)
- 37 wizard tests (26 validation + 11 integration)
- 11 metric browser tests
