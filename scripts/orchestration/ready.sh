#!/usr/bin/env bash
# Emit one JSON object per line ({number, title}) for "ready" issues with the
# given label (harness H1 #679; native work-graph H2 #692).
#
# An issue is ready when:
#   1. it has NO open PR closing it (in-flight), AND
#   2. it is NOT claimed by a worker (claim protocol — see claims.sh), AND
#   3. every blocker is CLOSED, AND
#   4. it is NOT operator-gated (needs-human-input label — a human owes an
#      action no machine lane can take; excluded symmetrically in every path
#      so READY_DRIFT compares like with like).
#
# Resolution order (H2 Design A, probe #691; graduated cutover per plan v2):
#   1. NATIVE  — one GraphQL query per cohort: blockedBy (dependency edges),
#                labels (claimed), closedByPullRequestsReferences (in-flight —
#                platform linkage, catches manually-linked PRs and ignores
#                mere mentions, unlike the 'Closes #N' text search).
#   2. beads   — when the bd DAG is initialized (Gas Town projection).
#   3. body-parse (DEPRECATED fallback) — awk over '## Blocked by' sections.
#                Deleted in P3 (#694) after the drift window.
#
# PA-residual (plan-review on #692): the probe verified the GraphQL FIELDS
# exist, not their connection shape — so any native query/shape failure falls
# through to 2/3 with a warning instead of breaking dispatch.
#
# READY_DRIFT=1: run native AND body-parse, diff the sets. Exit 0 clean,
# 1 mismatch, 2 native-unavailable (an evidence gap counts as failure for the
# #694 gate). Wired into ready-drift.yml (scheduled) and `just morning`.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

LABEL="${1:?usage: ready.sh <label>}"

# --- 1. Native (Design A): one GraphQL query per cohort --------------------
native_ready() {
  local label="$1" nwo owner repo resp
  nwo=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null) || return 1
  owner=${nwo%%/*}; repo=${nwo##*/}
  resp=$(gh api graphql \
    -f query='query($owner:String!,$repo:String!,$label:String!){
      repository(owner:$owner,name:$repo){
        issues(states:OPEN,labels:[$label],first:100){
          nodes{
            number
            title
            labels(first:20){nodes{name}}
            blockedBy(first:50){nodes{number state}}
            closedByPullRequestsReferences(first:20,includeClosedPrs:false){nodes{number}}
          }}}}' \
    -f owner="$owner" -f repo="$repo" -f label="$label" 2>/dev/null) || return 1
  # Shape guard (PA-residual): require the nodes array before trusting output.
  printf '%s' "$resp" | jq -e '.data.repository.issues.nodes' >/dev/null 2>&1 || return 1
  printf '%s' "$resp" | jq -c '
    .data.repository.issues.nodes[]
    | select(((.labels.nodes // []) | map(.name) | index("claimed")) | not)
    | select(((.labels.nodes // []) | map(.name) | index("needs-human-input")) | not)
    | select((.closedByPullRequestsReferences.nodes // []) | length == 0)
    | select(((.blockedBy.nodes // []) | map(select(.state == "OPEN")) | length) == 0)
    | {number, title}'
}

# --- 3. Body-parse fallback (deprecated — P3 deletes this) ------------------
legacy_ready() {
  local label="$1"
  local IN_FLIGHT CLAIMED HUMAN_GATED
  IN_FLIGHT=$(in_flight_numbers)
  CLAIMED=$(claimed_numbers)
  HUMAN_GATED=$(human_gated_numbers)
  excluded() {
    contains_num "$IN_FLIGHT" "$1" || contains_num "$CLAIMED" "$1" \
      || contains_num "$HUMAN_GATED" "$1"
  }

  gh issue list --label "$label" --state open --limit 200 --json number,title,body \
    | jq -c '.[]' \
    | while IFS= read -r issue; do
        num=$(echo "$issue" | jq -r '.number')
        excluded "$num" && continue
        body=$(echo "$issue" | jq -r '.body // ""')
        blockers=$(echo "$body" \
          | awk '/^## Blocked by/{flag=1; next} /^## /{flag=0} flag' \
          | grep -oE '#[0-9]+' | tr -d '#' | sort -u || true)
        ready=true
        for b in $blockers; do
          state=$(gh issue view "$b" --json state -q '.state' 2>/dev/null || echo "MISSING")
          if [ "$state" != "CLOSED" ]; then
            ready=false
            break
          fi
        done
        if [ "$ready" = "true" ]; then
          echo "$issue" | jq -c '{number, title}'
        fi
      done
}

# --- Drift mode: native vs body-parse ---------------------------------------
if [ "${READY_DRIFT:-0}" = "1" ]; then
  if ! NATIVE_OUT=$(native_ready "$LABEL"); then
    echo "READY_DRIFT: native path unavailable for '$LABEL' — evidence gap" >&2
    exit 2
  fi
  LEGACY_OUT=$(legacy_ready "$LABEL")
  A=$(printf '%s\n' "$NATIVE_OUT" | jq -r '.number' 2>/dev/null | sort -n || true)
  B=$(printf '%s\n' "$LEGACY_OUT" | jq -r '.number' 2>/dev/null | sort -n || true)
  if [ "$A" = "$B" ]; then
    echo "READY_DRIFT: clean — cohort '$LABEL' agrees ($(printf '%s' "$A" | grep -c '[0-9]' || true) ready)"
    exit 0
  fi
  echo "READY_DRIFT: MISMATCH for cohort '$LABEL'"
  echo "  native: $(printf '%s' "$A" | tr '\n' ' ')"
  echo "  legacy: $(printf '%s' "$B" | tr '\n' ' ')"
  exit 1
fi

# --- Normal resolution: native → beads → body-parse -------------------------
if OUT=$(native_ready "$LABEL"); then
  [ -n "$OUT" ] && printf '%s\n' "$OUT"
  exit 0
fi
echo "warning: native dependency query unavailable — falling back (#692 PA-residual)" >&2

# Prefer beads when initialized: true DAG semantics with cycle detection.
if command -v bd >/dev/null 2>&1 && bd list --all --json >/dev/null 2>&1; then
  IN_FLIGHT=$(in_flight_numbers)
  CLAIMED=$(claimed_numbers)
  HUMAN_GATED=$(human_gated_numbers)
  excluded() {
    contains_num "$IN_FLIGHT" "$1" || contains_num "$CLAIMED" "$1" \
      || contains_num "$HUMAN_GATED" "$1"
  }
  bd ready --label "$LABEL" --json --limit 200 2>/dev/null \
    | jq -c '.[] | select(.external_ref != null) | select(.external_ref | startswith("gh-")) | {number: (.external_ref | sub("^gh-"; "") | tonumber), title}' \
    | while IFS= read -r issue; do
        num=$(echo "$issue" | jq -r '.number')
        excluded "$num" && continue
        echo "$issue"
      done
  exit 0
fi

legacy_ready "$LABEL"
