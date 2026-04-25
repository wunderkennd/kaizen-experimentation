#!/usr/bin/env bash
set -euo pipefail

# Test Coverage Improvement Plan — GitHub Issues Bootstrap
#
# Creates 5 milestones, 6 new labels, and 31 issues from the spec at
# docs/coordination/test-coverage-improvement-plan.md.
#
# Run once. Idempotent: re-runs skip already-created milestones/labels and
# print "(exists)". Issue creation is NOT idempotent — running twice creates
# duplicate issues. Check `gh issue list --label test-coverage` first.
#
# Requires: gh CLI authenticated with repo write access.
#
# Usage:
#   ./scripts/create-test-coverage-issues.sh wunderkennd/kaizen-experimentation

REPO="${1:-}"
if [ -z "$REPO" ]; then
  echo "Usage: ./create-test-coverage-issues.sh owner/repo"
  exit 1
fi

# ──────────── Milestones ────────────

echo "=== Creating Test Coverage Milestones ==="

# Two-week cadence starting 2026-04-28. TC.0 must complete before TC.1–TC.4
# may start in parallel. Adjust due dates to match coordinator preference.
declare -A MS_DUE=(
  ["TC.0: Foundations"]="2026-05-12"
  ["TC.1: Statistical Goldens"]="2026-05-26"
  ["TC.2: Service Binaries"]="2026-06-09"
  ["TC.3: Contract Backfill"]="2026-06-23"
  ["TC.4: UI E2E + Hygiene"]="2026-07-07"
)

for ms in "TC.0: Foundations" "TC.1: Statistical Goldens" "TC.2: Service Binaries" "TC.3: Contract Backfill" "TC.4: UI E2E + Hygiene"; do
  due="${MS_DUE[$ms]}"
  gh api repos/"$REPO"/milestones -f title="$ms" -f due_on="${due}T00:00:00Z" -f state=open >/dev/null 2>&1 \
    && echo "  ✓ $ms (due $due)" \
    || echo "  (exists) $ms"
done

# ──────────── Labels ────────────

echo ""
echo "=== Creating Labels ==="

# agent-N + P0/P1/P2 already exist from create-phase5-issues.sh. Only add
# the test-coverage-specific labels here.
for label in "test-coverage" "sprint-tc-0" "sprint-tc-1" "sprint-tc-2" "sprint-tc-3" "sprint-tc-4"; do
  gh label create "$label" --repo "$REPO" >/dev/null 2>&1 \
    && echo "  ✓ $label" \
    || echo "  (exists) $label"
done

# ──────────── Helper ────────────

# Usage: tc_issue ID SPRINT PRIORITY AGENT ESTIMATE DEPENDS BRANCH_SLUG TITLE
#
# Issue body is intentionally compact — it points workers at the full spec
# in docs/coordination/test-coverage-improvement-plan.md. The autonomous-sprint
# recipe pipes `head -50` of the body into multiclaude worker create, so the
# 6-section body fits with margin.
tc_issue() {
  local id="$1" sprint="$2" priority="$3" agent="$4" estimate="$5" depends="$6" branch_slug="$7" title="$8"
  local milestone
  case "$sprint" in
    0) milestone="TC.0: Foundations" ;;
    1) milestone="TC.1: Statistical Goldens" ;;
    2) milestone="TC.2: Service Binaries" ;;
    3) milestone="TC.3: Contract Backfill" ;;
    4) milestone="TC.4: UI E2E + Hygiene" ;;
    *) echo "  ⚠ Unknown sprint: $sprint" >&2; return 1 ;;
  esac
  local labels="test-coverage,sprint-tc-${sprint},agent-${agent},${priority}"
  local branch="agent-${agent}/test/${branch_slug}"
  local body
  body=$(cat <<EOF
## Task ID
${id}

## Owner / Branch / Estimate
- **Agent**: Agent-${agent}
- **Priority**: ${priority}
- **Estimate**: ${estimate}
- **Depends on**: ${depends}
- **Branch**: \`${branch}\`

## Full Spec
The complete acceptance criteria, file list, and verify commands live in:
\`docs/coordination/test-coverage-improvement-plan.md\` → search for \`${id}\`.

The spec is the source of truth. Read it before starting.

## Done When
- [ ] All acceptance criteria from the spec are met (paste & check them as you go)
- [ ] PR title: \`test(crate): ${id} — ${title}\`
- [ ] PR body includes \`Closes #__this_issue__\`
- [ ] PR body includes coverage delta (llvm-cov / go cover / vitest) for affected paths
- [ ] CI green; reviewer approval per Coordination Protocol in the plan doc

## Worker Notes
- If you discover the spec is wrong or stale, comment on this Issue and add the \`blocked\` label rather than improvising.
- Cross-module contract tests require BOTH the producer and consumer agent to approve the PR.
EOF
)
  local num
  num=$(gh issue create --repo "$REPO" \
    --milestone "$milestone" \
    --title "${id}: ${title}" \
    --label "$labels" \
    --body "$body" 2>/dev/null | grep -o '[0-9]*$' || true)
  if [ -n "$num" ]; then
    echo "  ✓ #$num ${id}: ${title}"
  else
    echo "  ✗ FAILED to create ${id}: ${title}"
  fi
}

# ──────────── Sprint TC.0 — Foundations ────────────

echo ""
echo "=== Sprint TC.0 — Foundations ==="

tc_issue "TC-001" 0 "P0" 2 "M" "none"               "tc-001-llvm-cov"           "Wire cargo-llvm-cov into Rust CI"
tc_issue "TC-002" 0 "P0" 2 "S" "none"               "tc-002-go-ts-coverage"     "Wire go test -coverprofile and Vitest coverage"
tc_issue "TC-003" 0 "P1" 2 "S" "TC-001, TC-002"     "tc-003-codecov"            "Establish coverage baseline + Codecov integration"
tc_issue "TC-004" 0 "P0" 2 "M" "none"               "tc-004-nightly-integration" "Resurrect ignored Kafka roundtrip tests in nightly CI"
tc_issue "TC-005" 0 "P2" 2 "S" "TC-003"             "tc-005-jules-cron"         "Auto-schedule Jules test-coverage workflow weekly"

# ──────────── Sprint TC.1 — Statistical Goldens ────────────

echo ""
echo "=== Sprint TC.1 — Statistical Goldens ==="

tc_issue "TC-101" 1 "P0" 4 "L" "TC-001"             "tc-101-avlm-golden"         "AVLM golden fixtures (ADR-015) — HIGHEST PRIORITY"
tc_issue "TC-102" 1 "P0" 4 "L" "TC-001"             "tc-102-switchback-golden"   "Switchback golden fixtures (ADR-022)"
tc_issue "TC-103" 1 "P1" 4 "L" "TC-001"             "tc-103-synthetic-control-golden" "Synthetic control golden fixtures (ADR-023)"
tc_issue "TC-104" 1 "P1" 4 "M" "TC-001"             "tc-104-adaptive-n-golden"   "Adaptive sample size golden + tests (ADR-020)"
tc_issue "TC-105" 1 "P2" 4 "M" "TC-001"             "tc-105-portfolio-golden"    "Portfolio optimization golden (ADR-019)"
tc_issue "TC-106" 1 "P2" 4 "S" "TC-001"             "tc-106-mcc-extended-golden" "Multiple comparison correction golden"
tc_issue "TC-107" 1 "P1" 4 "M" "TC-001"             "tc-107-sequential-golden"   "Sequential mSPRT golden fixtures"
tc_issue "TC-108" 1 "P1" 4 "M" "none"               "tc-108-stats-proptest"      "Backfill proptest blocks for stats modules"

# ──────────── Sprint TC.2 — Service Binaries ────────────

echo ""
echo "=== Sprint TC.2 — Service Binaries ==="

tc_issue "TC-201" 2 "P0" 4 "L" "TC-001"             "tc-201-policy-core"         "LMAX policy core unit tests + integration suite (M4b)"
tc_issue "TC-202" 2 "P0" 7 "L" "TC-001"             "tc-202-flags-unit"          "experimentation-flags unit suite (M7)"
tc_issue "TC-203" 2 "P1" 5 "L" "TC-001"             "tc-203-management-grpc"     "experimentation-management grpc.rs + store.rs unit tests (M5)"
tc_issue "TC-204" 2 "P1" 1 "M" "TC-001"             "tc-204-assignment-service"  "assignment service.rs + config.rs unit tests (M1)"
tc_issue "TC-205" 2 "P2" 2 "S" "TC-001"             "tc-205-pipeline-kafka"      "pipeline kafka.rs unit tests (M2)"

# ──────────── Sprint TC.3 — Contract Backfill ────────────

echo ""
echo "=== Sprint TC.3 — Contract Backfill ==="

tc_issue "TC-301" 3 "P1" 2 "M" "TC-204"             "tc-301-m1m2-contract"       "M1↔M2 contract: Assignment → Pipeline event emission"
tc_issue "TC-302" 3 "P1" 4 "M" "TC-004"             "tc-302-m2m4a-contract"      "M2↔M4a contract: Pipeline Delta handoff"
tc_issue "TC-303" 3 "P1" 5 "M" "TC-202"             "tc-303-m5m7-contract"       "M5↔M7 contract: Flag-experiment linkage"
tc_issue "TC-304" 3 "P2" 1 "M" "TC-202, TC-204"     "tc-304-m7m1-contract"       "M7↔M1 contract: Flag-driven assignment"
tc_issue "TC-305" 3 "P1" 5 "M" "TC-201"             "tc-305-m4bm5-contract"      "M4b↔M5 contract: Auto-pause on guardrail breach"
tc_issue "TC-306" 3 "P1" 7 "M" "none"               "tc-306-sdk-hash-parity"     "SDK hash parity tests across all 5 client SDKs"

# ──────────── Sprint TC.4 — UI E2E + Hygiene ────────────

echo ""
echo "=== Sprint TC.4 — UI E2E + Hygiene ==="

tc_issue "TC-401" 4 "P1" 6 "L" "none"               "tc-401-playwright-e2e"      "Playwright smoke E2E suite for the experiment wizard"
tc_issue "TC-402" 4 "P2" 5 "M" "none"               "tc-402-migration-tests"     "SQL migration round-trip tests"
tc_issue "TC-403" 4 "P2" 7 "S" "TC-202, TC-203"     "tc-403-resolve-contract-todos" "Resolve in-tree TODO/FIXME contract test stubs"
tc_issue "TC-404" 4 "P1" 2 "S" "TC-003"             "tc-404-coverage-gates"      "Add coverage thresholds to PR gate"

# ──────────── Summary ────────────

echo ""
echo "=== Done ==="
echo ""
echo "Created 5 milestones, 6 labels, 31 issues (if not already present)."
echo ""
echo "Next steps:"
echo ""
echo "  1. Review the issue list:"
echo "     gh issue list --repo $REPO --label test-coverage --limit 50"
echo ""
echo "  2. Launch Sprint TC.0 (sequential, single agent):"
echo "     just evening tc.0"
echo ""
echo "  3. After TC.0 merges, launch later sprints in parallel:"
echo "     just evening tc.1   # Agent-4 task-parallel"
echo "     just evening tc.2   # Agents 1, 4, 5, 7 in parallel"
echo "     just evening tc.3   # Parallel pairs"
echo "     just evening tc.4   # Agents 5, 6"
echo ""
echo "  4. Track progress:"
echo "     docs/coordination/test-coverage-status.md"
