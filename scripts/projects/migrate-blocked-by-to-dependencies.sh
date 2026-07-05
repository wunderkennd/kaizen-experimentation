#!/usr/bin/env bash
# migrate-blocked-by-to-dependencies.sh — H2 P1 (#692)
#
# One-time migration: convert '## Blocked by' body references on OPEN issues
# into native dependency edges (probe #691 verified the REST surface).
#
#   DRY-RUN by default — prints the edge plan and a summary.
#   --apply           — creates the edges.
#
# Rules (plan v2, #680):
#   - Only edges whose blocker is currently OPEN are created; closed blockers
#     are satisfied history (logged, skipped).
#   - Same-repo '#N' references only; 'owner/repo#N' is reported as
#     unsupported (fleet cross-repo edges are H6 follow-up territory).
#   - Idempotent: an already-existing edge is a skip, not a failure.
#   - Body sections are left untouched — they stay as human narrative;
#     tooling stops parsing them at P3 (#694).
#
# Needs gh with issues: write (run via the workflow vehicle or locally).
set -uo pipefail

APPLY=0
[ "${1:-}" = "--apply" ] && APPLY=1

R=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo "repo: $R — mode: $([ "$APPLY" -eq 1 ] && echo APPLY || echo dry-run)"

# Map every issue (open+closed, PRs excluded) to state + database id.
declare -A STATE DBID
while IFS=$'\t' read -r num state dbid; do
  STATE[$num]=$state
  DBID[$num]=$dbid
done < <(gh api --paginate "repos/$R/issues?state=all&per_page=100" \
  --jq '.[] | select(.pull_request | not) | [.number, .state, .id] | @tsv')
echo "indexed ${#STATE[@]} issues"

CREATED=0; EXISTS=0; SKIP_CLOSED=0; SKIP_MISSING=0; XREPO=0; FAILED=0

while IFS= read -r row; do
  num=$(jq -r '.number' <<<"$row")
  body=$(jq -r '.body // ""' <<<"$row")
  section=$(awk '/^## Blocked by/{flag=1; next} /^## /{flag=0} flag' <<<"$body")
  [ -n "$section" ] || continue

  # Cross-repo references: report, don't attempt.
  while IFS= read -r ref; do
    [ -n "$ref" ] || continue
    XREPO=$((XREPO + 1))
    echo "  #$num: cross-repo ref '$ref' UNSUPPORTED (H6 follow-up)"
  done < <(grep -oE '[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#[0-9]+' <<<"$section" || true)

  for b in $(grep -oE '(^|[^/[:alnum:]])#[0-9]+' <<<"$section" | grep -oE '[0-9]+' | sort -un); do
    [ "$b" = "$num" ] && continue
    state=${STATE[$b]:-MISSING}
    if [ "$state" = "MISSING" ]; then
      SKIP_MISSING=$((SKIP_MISSING + 1))
      echo "  #$num blocked_by #$b: blocker not found — skip"
      continue
    fi
    if [ "$state" != "open" ]; then
      SKIP_CLOSED=$((SKIP_CLOSED + 1))
      echo "  #$num blocked_by #$b: blocker $state — satisfied history, skip"
      continue
    fi
    if [ "$APPLY" -eq 0 ]; then
      CREATED=$((CREATED + 1))
      echo "  #$num blocked_by #$b: WOULD create (dry-run)"
      continue
    fi
    ERR=$(gh api "repos/$R/issues/$num/dependencies/blocked_by" -X POST \
      -F issue_id="${DBID[$b]}" 2>&1 >/dev/null)
    rc=$?
    if [ $rc -eq 0 ]; then
      CREATED=$((CREATED + 1))
      echo "  #$num blocked_by #$b: created"
    elif grep -qiE 'already|duplicate|exist' <<<"$ERR"; then
      EXISTS=$((EXISTS + 1))
      echo "  #$num blocked_by #$b: already exists — skip"
    else
      FAILED=$((FAILED + 1))
      echo "  #$num blocked_by #$b: FAILED — $ERR"
    fi
  done
done < <(gh issue list --state open --limit 500 --json number,body --jq '.[] | @json')

echo ""
echo "summary: created=$CREATED exists=$EXISTS skipped-closed=$SKIP_CLOSED skipped-missing=$SKIP_MISSING cross-repo=$XREPO failed=$FAILED$([ "$APPLY" -eq 0 ] && echo ' (dry-run — nothing written)')"
[ "$FAILED" -eq 0 ]
