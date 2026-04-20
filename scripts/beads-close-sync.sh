#!/usr/bin/env bash
#
# Close beads whose linked GitHub Issue has been closed on GitHub.
#
# Walks every open bead with external_ref = "gh-*", checks the linked Issue's
# state, and closes the bead if the Issue is CLOSED. Safe to run repeatedly
# (no-op when nothing has changed). Intended for:
#   - `just morning` (catch overnight closures)
#   - git post-merge hook (catch PR-driven closures on pull)
#   - manual invocation after a PR merge
#
# Never writes to GitHub — the PR's "Closes #N" already records the outcome there.

set -euo pipefail

if ! command -v bd >/dev/null 2>&1; then
  exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "✗ gh CLI not found" >&2
  exit 1
fi

GIT_COMMON=$(git rev-parse --git-common-dir 2>/dev/null || true)
if [ -z "$GIT_COMMON" ]; then
  exit 0
fi
BEADS_ROOT="$(cd "$(dirname "$GIT_COMMON")" && pwd)/.beads"

if [ ! -d "$BEADS_ROOT" ]; then
  exit 0
fi

echo "=== Close-syncing beads whose GH Issue closed ==="

OPEN_BEADS=$(bd list --json 2>/dev/null \
  | jq -r '.[]
      | select(.external_ref != null)
      | select(.external_ref | startswith("gh-"))
      | select(.status != "closed")
      | "\(.id)\t\(.external_ref)"' \
  || true)

if [ -z "$OPEN_BEADS" ]; then
  echo "  No open beads with GitHub refs."
  exit 0
fi

CLOSED=0
while IFS=$'\t' read -r BEAD_ID REF; do
  [ -z "$BEAD_ID" ] && continue
  NUM="${REF#gh-}"

  STATE=$(gh issue view "$NUM" --json state -q '.state' 2>/dev/null || echo "")
  if [ "$STATE" = "CLOSED" ]; then
    REASON=$(gh issue view "$NUM" --json stateReason -q '.stateReason' 2>/dev/null || echo "COMPLETED")
    bd close "$BEAD_ID" --reason "GH Issue #$NUM closed ($REASON)" >/dev/null 2>&1 || true
    echo "  ✓ $BEAD_ID closed (mirrors #$NUM $REASON)"
    CLOSED=$((CLOSED + 1))
  fi
done < <(printf '%s\n' "$OPEN_BEADS")

echo ""
echo "Summary: closed=$CLOSED"

if [ "$CLOSED" -gt 0 ]; then
  bd export -o "$BEADS_ROOT/issues.jsonl" 2>/dev/null || true
fi
