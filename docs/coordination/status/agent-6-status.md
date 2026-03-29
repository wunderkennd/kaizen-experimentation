# Agent-6 Status — Phase 5

**Module**: M6 UI
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.3
Focus: Switchback results tab (ADR-022), Quasi-experiment / Synthetic Control results tab (ADR-023)
Branch: work/cool-tiger

Focus: ADR-011 multi-objective bandit reward visualization, ADR-012 LP constraint status table
Branch: work/fancy-koala

- [x] **EValueGauge component** (ADR-018)
  - `ui/src/components/e-value-gauge.tsx`
  - SVG semi-circle gauge (pure CSS/SVG, no Recharts dependency needed)
  - Color coding: red (rejected), yellow (e_value > 5), grey (insufficient evidence)
  - Displays: e-value on log scale, implied significance level (1/e_value), reject/no-reject
  - React.memo; dynamically imported in results page
  - Shown when `analysisResult.eValueResult` is present

- [x] **FdrBudgetBar component** (ADR-018)
  - `ui/src/components/fdr-budget-bar.tsx`
  - Progress bar: alphaWealth / initialWealth; orange warning when < 20% remains
  - Fetches `GetOnlineFdrState` from AnalysisService (best-effort, 404 = not applicable)
  - Shows: wealth remaining, numTested, numRejected, estimated FDR
  - React.memo; dynamically imported in results page
  - Shown when `experiment.onlineFdrConfig` is present

- [x] **Wire into results page**
  - `ui/src/app/experiments/[id]/results/page.tsx`
  - EValueGauge + FdrBudgetBar rendered after ResultsSummary in all tabs
  - Both dynamically imported (ssr: false)

- [x] **Types** (`ui/src/lib/types.ts`)
  - `EValueResult`: eValue, logEValue, impliedLevel, reject, alpha
  - `OnlineFdrConfig`: targetAlpha, initialWealth, strategy (E_LOND | E_BH)
  - `OnlineFdrState`: alphaWealth, initialWealth, numTested, numRejected, currentFdr
  - `Experiment.onlineFdrConfig?: OnlineFdrConfig`
  - `AnalysisResult.eValueResult?: EValueResult`

- [x] **API** (`ui/src/lib/api.ts`)
  - `getOnlineFdrState(experimentId)` → AnalysisService/GetOnlineFdrState
  - `adaptAnalysisResult` passes through `eValueResult`
  - `adaptExperiment` passes through `onlineFdrConfig`

- [x] **MSW mocks** (`ui/src/__mocks__/`)
  - `SEED_ONLINE_FDR_STATES` in seed-data.ts (experiment 111...: wealth=0.032/0.05)
  - `GetOnlineFdrState` handler in handlers.ts
  - Experiment 111... has `onlineFdrConfig` (E_LOND, α=0.05, initialWealth=0.05)
  - Analysis result 111... has `eValueResult` (eValue=12.5, reject=false)

- [x] `npm run build` passes (0 errors, 12/12 static pages)
- [x] Relevant tests pass (results-dashboard, avlm-adaptive-n, analysis-tabs all green)
  - Pre-existing flaky timing failures in performance.test.tsx (confirmed pre-existing on baseline)

## Blocked

None.

## Next Up

- Portfolio index page /portfolio (ADR-019)
- Enhanced bandit dashboard (ADR-016 slate bandit visualization)

## Completed (this sprint)

- [x] **SlateResultsPanel** (ADR-016)
  - `ui/src/components/slate/SlateResultsPanel.tsx`
  - Ranked ordered list with position badges (1..n) and per-slot probability badges
  - Color-coded probability bands: green ≥80%, blue ≥60%, indigo ≥40%, purple ≥20%, gray <20%
  - Shows overall slate probability in scientific notation (slateProbability)
  - `React.memo` wrapped

- [x] **SlatePositionBiasChart** (ADR-016)
  - `ui/src/components/slate/SlatePositionBiasChart.tsx`
  - Recharts BarChart showing per-position CTR from LIPS OPE estimate
  - Fetches `getSlateOpe(experimentId)` from AnalysisService/GetSlateOpe
  - Position opacity gradient (deeper positions appear fainter)
  - Shows policy value estimate from LIPS
  - `React.memo` wrapped, dynamically imported in experiment detail page

- [x] **SlateAssignmentForm** (ADR-016)
  - `ui/src/components/slate/SlateAssignmentForm.tsx`
  - Candidate item picker (textarea, comma-separated)
  - n_slots selector (1–20)
  - User ID field
  - Submit → calls AssignmentService/GetSlateAssignment
  - Renders SlateResultsPanel on successful response
  - Client-side validation: empty candidates, n_slots > candidate count
  - `React.memo` wrapped

- [x] **Slate tab on experiment detail page**
  - `ui/src/app/experiments/[id]/page.tsx` — Slate section added
  - Only visible when `experiment.type === 'SLATE'`
  - Tab label "Slate" with tab-nav chrome
  - Two-column layout: SlateAssignmentForm | SlatePositionBiasChart
  - Both components code-split via `next/dynamic`

- [x] **New types** (ADR-016)
  - `SlateAssignmentResponse`, `SlatePositionBiasPoint`, `SlateOpeResult` in `types.ts`
  - Added `SLATE` to `ExperimentType` union
  - Added `SLATE: 'Slate Bandit'` to `TYPE_LABELS` in `utils.ts`

- [x] **API functions** (ADR-016)
  - `getSlateAssignment()` → AssignmentService/GetSlateAssignment
  - `getSlateOpe()` → AnalysisService/GetSlateOpe
  - New `ASSIGNMENT_URL` / `ASSIGNMENT_SVC` constants

- [x] **Seed data and MSW handlers**
  - Seed experiment `cccccccc-cccc-cccc-cccc-cccccccccccc` (homepage_slate_v1, RUNNING)
  - `SEED_SLATE_OPE_RESULTS` with 10-position cascade bias data
  - MSW handlers: `GetSlateOpe` (ANALYSIS_SVC), `GetSlateAssignment` (ASSIGNMENT_SVC)

- [x] **Tests** (17 new tests, 0 regressions)
  - `ui/src/__tests__/slate-bandit.test.tsx`
  - SlateResultsPanel: 5 tests (items, positions, probability badges, overall prob, testid)
  - SlatePositionBiasChart: 4 tests (loading, chart, policy value, no-data message)
  - SlateAssignmentForm: 5 tests (form fields, testid, submit success, validation errors)
  - ExperimentDetailPage integration: 3 tests (tab visible for SLATE, hidden for AB)
  - Fixed 4 affected tests in proto-wire-format, experiment-list, monitoring

## Completed (Phase 5 — previous PRs)

## Completed (this sprint)

- [x] **Switchback experiment results tab** (ADR-022)
  - `ui/src/components/switchback-tab.tsx`
  - Block timeline: flex row of colored time-bands (indigo=treatment, gray=control)
  - Block-level outcome table: blockId, period dates, assignment badge, outcome, N
  - ACF carryover diagnostic chart: Recharts ComposedChart with ACF bars + dotted CI lines
  - RI null distribution histogram: BarChart binned into 20 buckets, ReferenceLine at observed ATE
  - Summary stats: ATE ± SE, RI p-value (formatPValue), block count (nT / nC), carryover badge
  - `React.memo` on AcfChart, RiHistogram, BlockOutcomeTable
  - Code-split via `next/dynamic` (ssr: false)
  - API: `getSwitchbackResult(experimentId)` → AnalysisService/GetSwitchbackResult
  - Visible only when `experiment.type === 'SWITCHBACK'`

- [x] **Quasi-experiment / Synthetic Control results tab** (ADR-023)
  - `ui/src/components/quasi-experiment-tab.tsx`
  - Treated vs Synthetic Control time series: Area CI band + dual Lines + treatment-start ReferenceLine
  - Pointwise treatment effects chart with y=0 ReferenceLine
  - Cumulative treatment effects chart (green line)
  - Donor weights table with inline progress bars
  - Placebo small-multiples grid: `PlaceboMiniChart` (memo, 100px) per donor
  - RMSPE diagnostic badge: colored green/amber/red by ratio, pre/post grid, p-value
  - `React.memo` on all chart sub-components and DonorWeightTable
  - Code-split via `next/dynamic` (ssr: false)
  - API: `getSyntheticControlResult(experimentId)` → AnalysisService/GetSyntheticControlResult
  - Visible only when `experiment.type === 'QUASI_EXPERIMENT'`

- [x] **Types extended** (`ui/src/lib/types.ts`)
  - `ExperimentType` union: added `'SWITCHBACK' | 'QUASI_EXPERIMENT'`
  - New interfaces: `SwitchbackBlock`, `SwitchbackAcfPoint`, `SwitchbackResult`
  - New interfaces: `SyntheticControlTimePoint`, `SyntheticControlEffect`, `DonorWeight`,
    `PlaceboTimeSeries`, `PlaceboResult`, `SyntheticControlResult`

- [x] **API functions** (`ui/src/lib/api.ts`)
  - `getSwitchbackResult(experimentId)` → `GetSwitchbackResult`
  - `getSyntheticControlResult(experimentId)` → `GetSyntheticControlResult`

- [x] **utils.ts**: `TYPE_LABELS` extended with `SWITCHBACK: 'Switchback'`, `QUASI_EXPERIMENT: 'Quasi-Experiment'`

- [x] **Results page** (`ui/src/app/experiments/[id]/results/page.tsx`)
  - `AnalysisTab` type extended with `'switchback' | 'quasi'`
  - Conditional tab entries for SWITCHBACK/QUASI_EXPERIMENT experiments
  - Tab panels for both new tabs

- [x] **Seed data** (`ui/src/__mocks__/seed-data.ts`)
  - 2 new experiments: `cccccccc...` (SWITCHBACK, CONCLUDED), `dddddddd...` (QUASI_EXPERIMENT, CONCLUDED)
  - `INITIAL_SWITCHBACK_RESULTS`: 12 blocks, 5 ACF lags, 500-point RI null distribution
  - `INITIAL_SYNTHETIC_CONTROL_RESULTS`: 105-day time series, 91-day effects, 5 donors, 5 placebos
  - MSW handlers: `GetSwitchbackResult`, `GetSyntheticControlResult`

- [x] **Tests**: 20 new tests in `switchback-quasi.test.tsx` (all passing)
  - Updated hardcoded seed counts in 3 existing test files (proto-wire-format, experiment-list, monitoring)

## Completed (previous sprints)

- [x] **AVLM confidence sequence boundary plot** (ADR-015)
  - `ui/src/components/charts/avlm-boundary-plot.tsx`
  - Recharts ComposedChart with Area (confidence sequence band) + dual Line (CUPED + raw estimate)
  - ReferenceLine at H0=0; conclusive badge when CS excludes zero
  - Dynamically imported; legacy alpha-spending chart preserved under details fold
  - API: `getAvlmResult(experimentId, metricId)` → AnalysisService/GetAvlmResult
  - Types: AvlmBoundaryPoint, AvlmResult

- [x] **AvlmSequencePlot component** (ADR-015)
  - `ui/src/components/AvlmSequencePlot.tsx`
  - Recharts ComposedChart with Area (confidence sequence band [lower, upper]) + Line (CUPED estimate)
  - ReferenceLine at H₀=0 (horizontal null line)
  - React.memo, no `any` types (TypeScript strict)
  - Fetches from `getAvlmResult(experimentId, metricId)`, handles 404 gracefully
  - Wired into results page: shown when `sequentialTestConfig.method === 'AVLM'`, wrapped in Suspense
  - Dynamic import with `ssr: false`

- [x] **AdaptiveNZoneBadge component** (ADR-020)
  - `ui/src/components/AdaptiveNZoneBadge.tsx`
  - Zone colors: FAVORABLE=green, PROMISING=yellow, FUTILE=red, INCONCLUSIVE=gray
  - Shows `recommended_n` when present (e.g. "Adaptive N: Promising · Rec. N: 150,000")
  - React.memo, pure display (takes `AdaptiveNResult` as prop)
  - Wired into results page below `ResultsSummary`, wrapped in Suspense, shown when `adaptiveN !== null`

- [x] **SequentialMethod type updated**
  - Added `'AVLM'` to `SequentialMethod` union in `ui/src/lib/types.ts`
  - Backward-compatible: no exhaustive checks, explicit array in metrics-step.tsx unchanged

- [x] **Results page wiring** (`ui/src/app/experiments/[id]/results/page.tsx`)
  - Dynamic imports for `AvlmSequencePlot` and `AdaptiveNZoneBadge` (ssr: false)
  - `<Suspense>` wrappers around both component usages
  - `AdaptiveNZoneBadge` shown globally below summary when adaptiveN data exists
  - `AvlmSequencePlot` shown per-metric in sequential section when method is AVLM

- [x] **Portfolio optimization dashboard** (ADR-019)

- [x] **Provider Health dashboard page** (ADR-014)

- [x] **Feedback loop analysis tab** (ADR-021)

- [x] **ADR-011 Multi-objective reward composition chart**

- [x] **ADR-012 LP constraint status table**

- [x] **E-value gauge + FDR budget bar** (ADR-018)

## Blocked

None.

## Next Up

- E-value display (ADR-018) — pending Agent-4 GetEvalueResult endpoint

## Completed (Phase 5 — prior sprints)

- [x] AVLM confidence sequence boundary plot (ADR-015)
  - `ui/src/components/charts/avlm-boundary-plot.tsx`
- [x] Adaptive N zone indicator badge + timeline (ADR-020)
  - `ui/src/components/adaptive-n-badge.tsx`, `adaptive-n-timeline.tsx`
- [x] Feedback loop analysis tab (ADR-021)
  - `ui/src/components/feedback-loop-tab.tsx`
- [x] /portfolio/provider-health page (ADR-014)
  - Time series charts, provider filter, MSW mock, 8 tests

- [x] AVLM confidence sequence boundary plot — `charts/avlm-boundary-plot.tsx` (PR: work/proud-eagle)
- [x] Adaptive N zone indicator badge — `adaptive-n-badge.tsx` (PR: work/proud-eagle)
- [x] Extended timeline visualization — `adaptive-n-timeline.tsx` (PR: work/proud-eagle)
- [x] Feedback loop analysis tab — `feedback-loop-tab.tsx` (PR: work/proud-eagle)

## Dependencies (wire-ready, awaiting backend)

- Agent-4: AnalysisService/GetAvlmResult, GetAdaptiveN, GetFeedbackLoopAnalysis, GetOnlineFdrState, GetSlateOpe
- Agent-4: AnalysisService/GetSwitchbackResult, GetSyntheticControlResult
- Agent-4: AnalysisService/GetEvalueResult (ADR-018, next sprint)
- Agent-1: AssignmentService/GetSlateAssignment
- Agent-2: Feedback loop retraining event data flow
- Agent-5: `ExperimentManagementService/GetPortfolioAllocation` gRPC endpoint
- Agent-4: BanditPolicyService objectiveBreakdowns and constraintStatuses in GetBanditDashboard response

## Notes

- `AvlmSequencePlot` is intentionally distinct from `AvlmBoundaryPlot` (from PR work/proud-eagle):
  - `AvlmBoundaryPlot`: shows CUPED + raw estimate dual-line chart, fetches for all sequential methods
  - `AvlmSequencePlot`: simpler confidence band + single estimate line, shown only when method=AVLM
- For AVLM experiments, both components render (AvlmSequencePlot first, AvlmBoundaryPlot second).
  Opportunity: collapse to single component in a future PR if product finds dual-charts redundant.
- `AdaptiveNZoneBadge` uses yellow for PROMISING (vs blue in `AdaptiveNBadge` from prior PR).
  Opportunity: align color scheme across both badge components in a future PR.
