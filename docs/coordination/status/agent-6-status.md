# Agent-6 Status — Phase 5

**Module**: M6 UI
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.2
Focus: E-value gauge (ADR-018), Online FDR budget bar (ADR-018)
Branch: work/fancy-platypus

## Completed (this PR)

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

## Completed (Phase 5 — previous PRs)

- [x] **AVLM confidence sequence boundary plot** (ADR-015)
  - `ui/src/components/charts/avlm-boundary-plot.tsx`
  - Recharts ComposedChart with Area (confidence sequence band) + dual Line (CUPED + raw estimate)
  - ReferenceLine at H0=0; conclusive badge when CS excludes zero
  - Dynamically imported; legacy alpha-spending chart preserved under details fold
  - API: `getAvlmResult(experimentId, metricId)` → AnalysisService/GetAvlmResult
  - Types: AvlmBoundaryPoint, AvlmResult

- [x] **Adaptive N zone indicator badge** (ADR-020)
  - `ui/src/components/adaptive-n-badge.tsx`
  - Zones: FAVORABLE (green), PROMISING (blue), FUTILE (red), INCONCLUSIVE (gray)
  - Mounted in experiment detail page header for RUNNING/CONCLUDED experiments
  - API: `getAdaptiveN(experimentId)` → AnalysisService/GetAdaptiveN

- [x] **Extended timeline visualization** (ADR-020 PROMISING zone)
  - `ui/src/components/adaptive-n-timeline.tsx`
  - Only rendered when zone === PROMISING in results page overview tab

- [x] **Feedback loop analysis tab**
  - `ui/src/components/feedback-loop-tab.tsx`
  - Visible for AB/MAB/CONTEXTUAL_BANDIT experiments
  - API: `getFeedbackLoopAnalysis(experimentId)` → AnalysisService/GetFeedbackLoopAnalysis

- [x] /portfolio/provider-health page (ADR-014)

## Dependencies (wire-ready, awaiting backend)

- Agent-4: AnalysisService/GetAvlmResult, GetAdaptiveN, GetFeedbackLoopAnalysis, GetOnlineFdrState
- Agent-2: Feedback loop retraining event data flow
