#!/usr/bin/env bash
#
# Forward-sync open GitHub Issues to Gas Town beads.
#
# Creates a bead for each open GitHub Issue matching the given label, with
# external_ref = "gh-<N>". Idempotent: existing beads are left alone (we only
# create new links, not update bodies — use `bd edit` if you need that).
#
# Usage:
#   scripts/beads-sync.sh sprint-5.1
#   scripts/beads-sync.sh --all
#
# Prerequisites:
#   - bd CLI installed (go install github.com/steveyegge/beads/cmd/bd@latest)
#   - .beads/ initialized (run `just beads-init`)
#   - gh CLI authenticated

set -euo pipefail

LABEL="${1:-}"
if [ -z "$LABEL" ]; then
  echo "Usage: $0 <sprint-label> | --all" >&2
  echo "  e.g., $0 sprint-5.1" >&2
  exit 1
fi

if ! command -v bd >/dev/null 2>&1; then
  echo "✗ bd CLI not found." >&2
  echo "  Install: go install github.com/steveyegge/beads/cmd/bd@latest" >&2
  exit 1
fi

# Resolve the .beads/ location — auto-discovery via bd works across worktrees,
# but the JSONL export needs an explicit path at the main repo root.
GIT_COMMON=$(git rev-parse --git-common-dir 2>/dev/null || true)
if [ -z "$GIT_COMMON" ]; then
  echo "✗ Not in a git repository." >&2
  exit 1
fi
BEADS_ROOT="$(cd "$(dirname "$GIT_COMMON")" && pwd)/.beads"

if [ ! -d "$BEADS_ROOT" ]; then
  echo "✗ .beads/ not initialized at $BEADS_ROOT" >&2
  echo "  Run: just beads-init" >&2
  exit 1
fi

echo "=== Syncing open GitHub Issues → Gas Town beads ==="

# Build set of existing gh- external refs (idempotency guard)
EXISTING_REFS=$(bd list --all --json 2>/dev/null \
  | jq -r '.[] | select(.external_ref != null) | select(.external_ref | startswith("gh-")) | .external_ref' \
  || true)

# Query Issues
if [ "$LABEL" = "--all" ]; then
  echo "Scope: all open Issues"
  ISSUES_JSON=$(gh issue list --state open --limit 100 \
    --json number,title,body,labels,assignees --jq '.[] | @json')
else
  echo "Scope: label=$LABEL"
  ISSUES_JSON=$(gh issue list --label "$LABEL" --state open --limit 100 \
    --json number,title,body,labels,assignees --jq '.[] | @json')
fi

if [ -z "$ISSUES_JSON" ]; then
  echo "  No open Issues found."
  exit 0
fi

CREATED=0
SKIPPED=0

# Use process substitution so counters survive the loop (no subshell)
while IFS= read -r line; do
  [ -z "$line" ] && continue
  NUM=$(echo "$line" | jq -r '.number')
  TITLE=$(echo "$line" | jq -r '.title')
  BODY=$(echo "$line" | jq -r '.body // ""')
  LABELS=$(echo "$line" | jq -r '[.labels[].name] | join(",")')
  REF="gh-$NUM"

  if echo "$EXISTING_REFS" | grep -qx "$REF"; then
    echo "  ⊝ #$NUM already linked"
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # Create the bead. Body via stdin to avoid arg-length issues on long bodies.
  BEAD_ID=$(printf '%s' "$BODY" | bd create "$TITLE" \
    --external-ref "$REF" \
    --body-file - \
    --json 2>/dev/null \
    | jq -r '.id')

  if [ -z "$BEAD_ID" ] || [ "$BEAD_ID" = "null" ]; then
    echo "  ✗ Failed to create bead for #$NUM" >&2
    continue
  fi

  # Mirror sprint/agent/cluster labels onto the bead for bd queries
  if [ -n "$LABELS" ]; then
    IFS=',' read -ra LBL_ARR <<< "$LABELS"
    for l in "${LBL_ARR[@]}"; do
      bd label add "$BEAD_ID" "$l" >/dev/null 2>&1 || true
    done
  fi

  echo "  ✓ #$NUM → $BEAD_ID"
  CREATED=$((CREATED + 1))
done < <(printf '%s\n' "$ISSUES_JSON")

echo ""
echo "Summary: created=$CREATED already-linked=$SKIPPED"

# Export to git-tracked JSONL so teammates can `bd import` after pull
if [ "$CREATED" -gt 0 ]; then
  bd export -o "$BEADS_ROOT/issues.jsonl" 2>/dev/null || true
  echo "Exported to $BEADS_ROOT/issues.jsonl (commit to share with team)"
fi
