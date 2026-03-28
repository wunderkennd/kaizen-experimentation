# Agent-6 Status — Phase 5

**Module**: M6 UI
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.2
Focus: Portfolio optimization dashboard (ADR-019) + Provider Health dashboard (ADR-014)
Branch: work/gentle-panda, work/gentle-penguin

Focus: ADR-011 multi-objective bandit reward visualization, ADR-012 LP constraint status table
Branch: work/fancy-koala

## Completed (this sprint)

- [x] **Portfolio optimization dashboard** (ADR-019)
  - `ui/src/app/portfolio/page.tsx` — `PortfolioDashboard` page with code-split, data fetch, error/loading states
  - `ui/src/components/experiment-portfolio-table.tsx` — sortable table (name, effect_size, variance, allocated_traffic_pct, priority_score)
  - `ui/src/components/charts/budget-allocation-chart.tsx` — stacked horizontal bar chart (Recharts), `React.memo`
  - `ui/src/components/conflict-badge.tsx` — highlights experiments sharing user segments
  - Nav link updated: `/portfolio/provider-health` → `/portfolio`
  - API: `getPortfolioAllocation()` → `MGMT_SVC/GetPortfolioAllocation`
  - Types: `PortfolioExperiment`, `PortfolioAllocationResult`
  - Seed data: 4 realistic experiments with overlapping segments for conflict detection
  - Tests: 10 new tests, all passing. Zero regressions (510 total pass).

- [x] **Provider Health dashboard page** (ADR-014)
  - `ui/src/app/portfolio/provider-health/page.tsx` — `ProviderHealthPage` component
  - Three Recharts LineChart time series: `catalog_coverage_rate`, `provider_gini_coefficient`, `longtail_impression_share`
  - Provider dropdown filter — re-fetches all charts on change
  - Code-split via `next/dynamic` (ssr:false, ChartSkeleton loading state)
  - `React.memo` on all three chart components (`CatalogCoverageChart`, `ProviderGiniChart`, `LongTailImpressionChart`)
  - `ui/src/components/charts/provider-health-charts.tsx` — shared `ProviderMetricChartInner` + three memoized exports
  - Data fetching: `getProviderHealth(providerId?)` → `MetricComputationService/GetProviderHealth`
  - Types: `ProviderHealthPoint`, `ProviderHealthSeries`, `ProviderInfo`, `ProviderHealthResult`
  - MSW handler + seed data (2 providers × 2 experiments × 14 daily points)
  - 8 tests all passing; 499 total, 0 regressions

## Completed (previous PRs)

- [x] **ADR-011 Multi-objective reward composition chart** (2026-03-24, work/fancy-koala)
  - `ui/src/components/RewardCompositionChart.tsx`
  - Stacked BarChart (recharts) showing each objective's weighted contribution per arm
  - Color-coded segments: one per RewardObjective metricId
  - Footer shows primary objective and all weights
  - Empty state when no breakdowns or objectives provided
  - `React.memo` wrapped, strict TypeScript

- [x] **ADR-012 LP constraint status table** (2026-03-24, work/fancy-koala)
  - `ui/src/components/ConstraintStatusTable.tsx`
  - Table: constraint name, current value, limit, SATISFIED/VIOLATED badge
  - Red row highlight (`bg-red-50`) on VIOLATED rows
  - Red badge and bold current value for violated constraints
  - Empty state when no constraints configured
  - `React.memo` wrapped, strict TypeScript

- [x] **Wired into bandit dashboard page**
  - `ui/src/app/experiments/[id]/bandit/page.tsx` updated
  - `RewardCompositionChart` and `ConstraintStatusTable` sections rendered
  - Conditional: only shown when `banditExperimentConfig.rewardObjectives?.length > 0`
  - `ConstraintStatusTable` additionally guarded by `constraintStatuses?.length > 0`

- [x] **Types extended** (`ui/src/lib/types.ts`)
  - `RewardCompositionMethod` (WEIGHTED_SCALARIZATION | EPSILON_CONSTRAINT | TCHEBYCHEFF)
  - `RewardObjective` (metricId, weight, floor, isPrimary)
  - `BanditArmConstraint` (armId, minFraction, maxFraction)
  - `BanditGlobalConstraint` (label, coefficients, rhs)
  - `ArmObjectiveBreakdown` (armId, armName, objectiveContributions, composedReward)
  - `ConstraintStatus` (label, currentValue, limit, isSatisfied)
  - `BanditExperimentConfig` extended with optional: rewardObjectives, compositionMethod, armConstraints, globalConstraints
  - `BanditDashboardResult` extended with optional: objectiveBreakdowns, constraintStatuses

- [x] **Seed data updated** (`ui/src/__mocks__/seed-data.ts`)
  - cold_start_bandit (444...) banditExperimentConfig: 3 rewardObjectives, WEIGHTED_SCALARIZATION, 2 globalConstraints
  - BanditDashboardResult: objectiveBreakdowns for all 4 arms, 2 constraintStatuses (1 satisfied, 1 violated)

- [x] **Tests: 15 new tests, all passing** (26 total in bandit-dashboard.test.tsx)
  - 5 integration tests on bandit dashboard page (multi-objective sections, constraint badges)
  - 4 unit tests for RewardCompositionChart (empty states, aria label, footer)
  - 6 unit tests for ConstraintStatusTable (badges, red highlight, empty state, columns)
  - Pre-existing test isolation failures (performance, chaos-resilience, MSW state bleed) unchanged

- [x] `npm run build` passes
- [x] 26 / 26 bandit dashboard tests pass

## Blocked

None.

## Next Up

- E-value display (ADR-018) — pending Agent-4 GetEvalueResult endpoint
- Enhanced bandit dashboard (ADR-016 slate bandit visualization)

## Completed (Phase 5 — prior sprints)

- [x] AVLM confidence sequence boundary plot (ADR-015)
  - `ui/src/components/charts/avlm-boundary-plot.tsx`
- [x] Adaptive N zone indicator badge + timeline (ADR-020)
  - `ui/src/components/adaptive-n-badge.tsx`, `adaptive-n-timeline.tsx`
- [x] Feedback loop analysis tab (ADR-021)
  - `ui/src/components/feedback-loop-tab.tsx`
- [x] /portfolio/provider-health page (ADR-014)
  - Time series charts, provider filter, MSW mock, 8 tests
- [x] AVLM confidence sequence boundary plot (ADR-015)
- [x] Adaptive N zone indicator badge + extended timeline (ADR-020)
- [x] Feedback loop analysis tab (ADR-019 interference)

## Dependencies (wire-ready, awaiting backend)

- Agent-5: `ExperimentManagementService/GetPortfolioAllocation` gRPC endpoint
  - Request: `{}` (empty)
  - Response: `{ experiments: PortfolioExperiment[], totalAllocatedPct: float, computedAt: timestamp }`
  - `PortfolioExperiment` fields: `experiment_id`, `name`, `effect_size`, `variance`, `allocated_traffic_pct`, `priority_score`, `user_segments`

- Agent-4: BanditPolicyService objectiveBreakdowns and constraintStatuses in GetBanditDashboard response
- Agent-4: AnalysisService/GetAvlmResult, GetAdaptiveN, GetFeedbackLoopAnalysis
