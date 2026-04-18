#!/usr/bin/env bash
set -euo pipefail

# Create GitHub Issues for the SFD requirements gaps:
# 1. ADR-027 TOST Equivalence Testing
# 2. Heartbeat Sessionization
# 3. EBVS Detection
#
# Usage: ./create-sfd-gap-issues.sh owner/repo

REPO="${1:-}"
if [ -z "$REPO" ]; then
  echo "Usage: ./create-sfd-gap-issues.sh owner/repo"
  exit 1
fi

MS="Sprint 5.1: Measurement Foundations"

echo "=== Creating SFD Gap Issues ==="

# ADR-027: TOST Equivalence Testing (split across sprints)
gh issue create --repo "$REPO" \
  --milestone "$MS" \
  --title "ADR-027: TOST Equivalence Testing — Core Implementation" \
  --label "P1,agent-4,cluster-b" \
  --body "## Summary
Implement Two One-Sided Tests (TOST) for proving statistical equivalence in infrastructure migration experiments.

## Specification
Read \`docs/adrs/027-tost-equivalence-testing.md\`

## Acceptance Criteria
- [ ] \`tost.rs\`: \`tost_equivalence_test()\` with Welch's t internals
- [ ] \`tost_cuped_equivalence_test()\`: TOST composed with CUPED variance reduction
- [ ] \`tost_sample_size()\`: power analysis for equivalence designs
- [ ] Proto: \`EquivalenceTestConfig\` message with \`delta\`, \`delta_relative\`, \`alpha\`
- [ ] Golden-file: R TOSTER package to 6 decimal places
- [ ] Proptest: TOST p-value ≥ max(p_lower, p_upper); CI ⊂ [-δ,+δ] ↔ equivalent==true
- [ ] \`cargo test -p experimentation-stats\` passes

## Agent
Agent-4

## ADR
ADR-027"
echo "  ✓ ADR-027: TOST core"

gh issue create --repo "$REPO" \
  --milestone "$MS" \
  --title "ADR-027: TOST — M5 Validation + M6 Equivalence Results View" \
  --label "P1,agent-5,agent-6,cluster-b" \
  --body "## Summary
M5 validation for equivalence experiments + M6 equivalence-specific results dashboard.

## Specification
Read \`docs/adrs/027-tost-equivalence-testing.md\` Sections 5–6

## Acceptance Criteria
- [ ] M5: validate delta > 0, delta_relative only for MEAN/RATIO metrics, power warning at creation
- [ ] M5: conclusion logic — 'equivalent → safe to migrate' instead of 'reject → ship treatment'
- [ ] M6: CI plot with [-δ, +δ] equivalence margin shaded region
- [ ] M6: Green/Yellow/Red badge (Equivalent / Inconclusive / Not Equivalent)
- [ ] M6: power indicator at current sample size

## Agent
Agent-5 (validation) + Agent-6 (UI)

## ADR
ADR-027

## Blocked By
ADR-027 core implementation (Agent-4) must land first"
echo "  ✓ ADR-027: M5/M6 integration"

# Heartbeat Sessionization
gh issue create --repo "$REPO" \
  --milestone "$MS" \
  --title "Heartbeat Sessionization — 10s Heartbeat to PlaybackMetrics Aggregation" \
  --label "P1,agent-2,cluster-a" \
  --body "$(cat docs/issues/heartbeat-sessionization.md 2>/dev/null || cat /mnt/user-data/outputs/issues/heartbeat-sessionization.md 2>/dev/null || echo 'See docs/issues/heartbeat-sessionization.md for full spec')"
echo "  ✓ Heartbeat Sessionization"

# EBVS Detection
gh issue create --repo "$REPO" \
  --milestone "$MS" \
  --title "EBVS Detection — Exit Before Video Start Classification" \
  --label "P1,agent-2,agent-3,agent-4,agent-6" \
  --body "$(cat docs/issues/ebvs-detection.md 2>/dev/null || cat /mnt/user-data/outputs/issues/ebvs-detection.md 2>/dev/null || echo 'See docs/issues/ebvs-detection.md for full spec')"
echo "  ✓ EBVS Detection"

echo ""
echo "=== Done ==="
echo "4 issues created in $MS"
echo ""
echo "View:"
echo "  gh issue list --repo $REPO --milestone '$MS'"
