#!/usr/bin/env bash
set -euo pipefail

# Phase 5 GitHub Issues Bootstrap
# Run once to create milestones, issues, and sub-issues for all 6 sprints.
# Requires: gh CLI authenticated with repo write access.

REPO="${1:-}"
if [ -z "$REPO" ]; then
  echo "Usage: ./create-phase5-issues.sh owner/repo"
  exit 1
fi

echo "=== Creating Phase 5 Milestones ==="

for sprint in 0 1 2 3 4 5; do
  case $sprint in
    0) title="Sprint 5.0: Schema & Foundations"; due="2026-04-15" ;;
    1) title="Sprint 5.1: Measurement Foundations"; due="2026-05-06" ;;
    2) title="Sprint 5.2: Statistical Core"; due="2026-05-27" ;;
    3) title="Sprint 5.3: Constraints & New Experiment Types"; due="2026-06-17" ;;
    4) title="Sprint 5.4: Slate Bandits & Meta-Experiments"; due="2026-07-08" ;;
    5) title="Sprint 5.5: Advanced & Integration"; due="2026-07-29" ;;
  esac
  gh api repos/"$REPO"/milestones -f title="$title" -f due_on="${due}T00:00:00Z" -f state=open \
    2>/dev/null && echo "  ✓ $title" || echo "  (exists) $title"
done

echo ""
echo "=== Creating Labels ==="

for label in "agent-1" "agent-2" "agent-3" "agent-4" "agent-5" "agent-6" "agent-7" \
             "P0" "P1" "P2" "P3" "P4" "blocked" "cluster-a" "cluster-b" "cluster-c" \
             "cluster-d" "cluster-e" "cluster-f"; do
  gh label create "$label" --repo "$REPO" 2>/dev/null && echo "  ✓ $label" || echo "  (exists) $label"
done

echo ""
echo "=== Creating Sprint 5.0 Issues ==="

# Helper: create issue, capture number, create sub-issues
create_issue() {
  local milestone="$1" title="$2" body="$3" labels="$4" assignee="$5"
  shift 5
  local sub_issues=("$@")

  num=$(gh issue create --repo "$REPO" \
    --milestone "$milestone" \
    --title "$title" \
    --body "$body" \
    --label "$labels" \
    --assignee "$assignee" \
    2>/dev/null | grep -o '[0-9]*$')

  echo "  ✓ #$num $title"

  for sub in "${sub_issues[@]}"; do
    sub_num=$(gh issue create --repo "$REPO" \
      --milestone "$milestone" \
      --title "$sub" \
      --body "Sub-issue of #$num" \
      --label "$labels" \
      --assignee "$assignee" \
      2>/dev/null | grep -o '[0-9]*$')
    echo "    ↳ #$sub_num $sub"
  done
}

# --- Sprint 5.0: Schema & Foundations ---

MS="Sprint 5.0: Schema & Foundations"

gh issue create --repo "$REPO" --milestone "$MS" \
  --title "ADR-015: AVLM Implementation (Phase 1)" \
  --label "P0,agent-4,cluster-b" \
  --body "## Summary
Implement Anytime-Valid Linear Model in \`crates/experimentation-stats/src/avlm.rs\`.

## Specification
Read \`docs/adrs/015-anytime-valid-regression-adjustment.md\`

## Acceptance Criteria
- [ ] \`AvlmSequentialTest\` struct with 6 running sufficient statistics (sum_x, sum_y, sum_xy, sum_xx, sum_yy, n)
- [ ] O(1) \`update()\` per observation
- [ ] \`confidence_sequence()\` returns (estimate, lower, upper)
- [ ] \`is_significant()\` checks null exclusion from CS
- [ ] GROW/REGROW mixing boundary with configurable rho (default: unit-information prior)
- [ ] Golden-file tests against R \`avlm\` package to 4 decimal places
- [ ] Proptest invariant: CS covers true parameter at rate ≥ (1-α) over 10K sims
- [ ] \`pub mod avlm\` added to \`lib.rs\`
- [ ] \`cargo test -p experimentation-stats\` passes

## Agent
Agent-4 (Statistical Analysis)

## ADR
ADR-015"
echo "  ✓ ADR-015: AVLM Implementation"

gh issue create --repo "$REPO" --milestone "$MS" \
  --title "ADR-017: TC/JIVE Surrogate Calibration Fix (Phase 1)" \
  --label "P0,agent-4,cluster-c" \
  --body "## Summary
Replace R²-based surrogate calibration with Jackknife IV Estimation in \`crates/experimentation-stats/src/orl.rs\`.

## Specification
Read \`docs/adrs/017-offline-rl-long-term-effects.md\`

## Acceptance Criteria
- [ ] \`SurrogateCalibrator\` struct with K-fold cross-fold procedure
- [ ] For each fold k: train on K-1 folds, predict on held-out fold
- [ ] Cross-fold predictions used as instruments in 2SLS regression
- [ ] Output: treatment_effect_correlation (TC), jive_coefficient, jive_r_squared
- [ ] Update \`SurrogateModelConfig\` proto with JIVE fields
- [ ] Golden-file: reproduce Netflix KDD 2024 Table 2 results
- [ ] \`cargo test -p experimentation-stats\` passes

## Agent
Agent-4 (Statistical Analysis)

## ADR
ADR-017 Phase 1"
echo "  ✓ ADR-017: TC/JIVE"

gh issue create --repo "$REPO" --milestone "$MS" \
  --title "ADR-024: M7 Rust Port — Scaffold + CRUD (Phase 1)" \
  --label "P0,agent-7,cluster-f" \
  --body "## Summary
Create \`crates/experimentation-flags/\` and implement flag CRUD RPCs in Rust.

## Specification
Read \`docs/adrs/024-m7-rust-port.md\`

## Acceptance Criteria
- [ ] \`crates/experimentation-flags/\` added to workspace \`Cargo.toml\`
- [ ] tonic service with tonic-web for JSON HTTP mode
- [ ] sqlx PostgreSQL (async, compile-time checked queries)
- [ ] Migrate 5 SQL migrations from Go \`database/sql\` to sqlx
- [ ] Flag CRUD RPCs: CreateFlag, GetFlag, ListFlags, UpdateFlag, DeleteFlag
- [ ] Wire-format contract test: JSON output matches Go M7 for CreateFlag + GetFlag
- [ ] \`cargo test -p experimentation-flags\` passes

## Agent
Agent-7 (Feature Flags)

## ADR
ADR-024 Phase 1"
echo "  ✓ ADR-024: M7 Scaffold"

gh issue create --repo "$REPO" --milestone "$MS" \
  --title "ADR-018: E-Value Computation (Phase 1)" \
  --label "P1,agent-4,cluster-b" \
  --body "## Summary
Implement e-value computation alongside p-values in \`crates/experimentation-stats/src/evalue.rs\`.

## Specification
Read \`docs/adrs/018-e-value-framework-online-fdr.md\`

## Acceptance Criteria
- [ ] \`e_value_grow()\`: GROW martingale e-value for two-sample mean comparison
- [ ] \`e_value_avlm()\`: regression-adjusted e-value (pairs with AVLM)
- [ ] \`EValueResult { e_value, log_e_value, implied_level }\`
- [ ] SQL migration: \`ALTER TABLE metric_results ADD COLUMN e_value DOUBLE PRECISION, ADD COLUMN log_e_value DOUBLE PRECISION\`
- [ ] Golden-file: Ramdas/Wang monograph examples to 6 decimal places
- [ ] Proptest: e-values non-negative; product under null has E[e] ≤ 1
- [ ] \`cargo test -p experimentation-stats\` passes

## Agent
Agent-4 (Statistical Analysis)

## ADR
ADR-018 Phase 1"
echo "  ✓ ADR-018: E-Values"

gh issue create --repo "$REPO" --milestone "$MS" \
  --title "Phase 5 Proto Schema Extensions" \
  --label "P0,agent-4,cluster-a,cluster-b,cluster-c,cluster-d,cluster-e,cluster-f" \
  --body "## Summary
Land all Phase 5 proto schema extensions. This **blocks all other Phase 5 work**.

## Specification
Read \`design_doc_v7.0.md\` Section 3.6

## Changes Required
- [ ] \`ExperimentType\` += META(9), SWITCHBACK(10), QUASI(11)
- [ ] Bandit: \`RewardObjective\`, \`RewardConstraint\`, \`RewardCompositionMethod\`, \`ArmConstraint\`, \`GlobalConstraint\`, \`SlateConfig\`, \`PositionBiasModel\`, \`SlateInteractionModel\`, \`mad_randomization_fraction\`
- [ ] Metric: \`MetricStakeholder\`, \`MetricAggregationLevel\` enums, \`VarianceReductionConfig\`
- [ ] Analysis: \`SequentialMethod\` += AVLM(4), \`SyntheticControlMethod\` enum
- [ ] Management: \`AdaptiveSampleSizeConfig\`, \`ExperimentLearning\`, \`AnnualizedImpact\`, \`MetaExperimentConfig\`, \`SwitchbackConfig\`, \`QuasiExperimentConfig\`
- [ ] Event: \`ModelRetrainingEvent\`
- [ ] Assignment: \`GetSlateAssignment\` RPC
- [ ] Interference: feedback loop fields on \`InterferenceAnalysisResult\`
- [ ] \`buf lint proto/\` passes
- [ ] \`buf breaking proto/ --against .git#branch=main\` passes

## Agent
Agent-4 (owns proto, but all agents are downstream consumers)

## Blocks
All other Phase 5 issues"
echo "  ✓ Phase 5 Proto Schema Extensions"

echo ""
echo "=== Sprint 5.0 Issues Created ==="
echo ""
echo "Remaining sprints (5.1–5.5) can be created by running:"
echo "  ./create-phase5-issues.sh $REPO --sprint 1"
echo "  (or create manually using the same pattern)"
echo ""
echo "View your board:"
echo "  gh issue list --repo $REPO --milestone 'Sprint 5.0: Schema & Foundations'"
