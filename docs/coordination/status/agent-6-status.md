# Agent-6 Status — Phase 5

**Module**: M6 UI
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.2
Focus: META experiment UI components (ADR-013)
Branch: work/nice-tiger

## Completed (this PR)

- [x] **MetaExperimentConfig panel** (ADR-013)
  - `ui/src/components/meta/MetaExperimentConfig.tsx`
  - React.memo, shows variant-to-bandit-config mapping table (variant_id, bandit_type, arms)
  - Columns: Variant name, Variant ID, Bandit Type (badge), Arms (pill badges), Traffic %
  - Handles unconfigured variants (shows dash placeholders)

- [x] **MetaVariantSelector** (ADR-013)
  - `ui/src/components/meta/MetaVariantSelector.tsx`
  - React.memo, dropdown showing variants with associated bandit policy annotations
  - Format: `{name} [{algorithm abbreviation}, {n} arms]` or `[no policy]`
  - Props: variants, metaConfig, selectedVariantId, onChange, id, label

- [x] **TwoLevelIPWBadge** (ADR-013)
  - `ui/src/components/meta/TwoLevelIPWBadge.tsx`
  - React.memo, displays compound assignment probability P(variant) × P(arm|variant)
  - Shows 4-decimal compound value; title attribute exposes full breakdown
  - Wired into experiment detail page header for META experiments

- [x] **MetaConfigForm** (ADR-013)
  - `ui/src/components/meta/MetaConfigForm.tsx`
  - Per-variant bandit algorithm dropdown + comma-separated arms input
  - Uses WizardContext `metaConfig` field (keyed by variantId)

- [x] **Wiring: experiment creation form**
  - `EXPERIMENT_TYPE_META` added to ExperimentType union in `lib/types.ts`
  - `META: 'Meta Experiment'` added to `TYPE_LABELS` in `lib/utils.ts`
  - `META` added to EXPERIMENT_TYPES list in `basics-step.tsx`
  - `type-config-step.tsx`: META case renders `<MetaConfigForm>` via Next.js `dynamic()` (code-split)
  - `experiment-form.tsx`: META submits `metaConfig` in CreateExperimentRequest
  - `wizard-context.tsx`: `metaConfig: MetaConfig` added to WizardState with empty default

- [x] **Wiring: experiment detail page**
  - `MetaExperimentConfig` panel shown after Variants section when `type === 'META'`
  - `TwoLevelIPWBadge` shown in header when `type === 'META'` and `metaConfig` present
  - Both imports added to `experiments/[id]/page.tsx`

- [x] **Types**
  - `VariantBanditConfig` interface: `{ variantId, banditType: BanditAlgorithm, arms: string[] }`
  - `MetaConfig` interface: `{ variantBanditConfigs: VariantBanditConfig[] }`
  - `metaConfig?: MetaConfig` added to `Experiment` and `CreateExperimentRequest`

- [x] **Validation**
  - `validateMetaConfig()` in `lib/validation.ts`
  - META case added to `validateTypeConfig()` dispatcher
  - Validates: at least one config defined; each config has at least one arm

- [x] Tests: 21 new tests all passing, 0 regressions (520 total, 6 pre-existing skips)
  - MetaExperimentConfig: 6 tests
  - MetaVariantSelector: 5 tests
  - TwoLevelIPWBadge: 4 tests
  - validateMetaConfig: 4 tests
  - META wizard integration: 2 tests

## Blocked

None.

## Next Up

- E-value display (ADR-018) — pending Agent-4 GetEvalueResult endpoint
- Portfolio index page /portfolio (ADR-019)
- Enhanced bandit dashboard (ADR-016 slate bandit visualization)

## Completed (Phase 5 — previous PRs)

- [x] /portfolio/provider-health page (ADR-014)
  - Time series charts, provider filter, MSW mock, 8 tests

- [x] AVLM confidence sequence boundary plot (ADR-015)
  - `ui/src/components/charts/avlm-boundary-plot.tsx`

- [x] Adaptive N zone indicator badge (ADR-020)
  - `ui/src/components/adaptive-n-badge.tsx`

- [x] Extended timeline visualization (ADR-020 PROMISING zone)
  - `ui/src/components/adaptive-n-timeline.tsx`

- [x] Feedback loop analysis tab
  - `ui/src/components/feedback-loop-tab.tsx`

## Dependencies (wire-ready, awaiting backend)

- Agent-4: AnalysisService/GetAvlmResult, GetAdaptiveN, GetFeedbackLoopAnalysis
- Agent-1/4: MetaExperiment assignment probability endpoint (for live TwoLevelIPWBadge data)
- Agent-2: Feedback loop retraining event data flow
