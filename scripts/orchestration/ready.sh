#!/usr/bin/env bash
# Emit one JSON object per line ({number, title}) for "ready" issues with the
# given label (harness H1, #679; extracted from the justfile `_ready` recipe).
#
# An issue is ready when:
#   1. it has NO open PR closing it (in-flight), AND
#   2. it is NOT claimed by a worker (claim protocol — see claims.sh), AND
#   3. every "#N" under its "## Blocked by" body section is CLOSED
#      (or beads says so, when the bd DAG is initialized — preferred path).
#
# H2 (#680) replaces path 3's body parsing with native issue dependencies via
# GraphQL; the claimed/in-flight predicates stay.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

LABEL="${1:?usage: ready.sh <label>}"

IN_FLIGHT=$(in_flight_numbers)
CLAIMED=$(claimed_numbers)

excluded() { contains_num "$IN_FLIGHT" "$1" || contains_num "$CLAIMED" "$1"; }

# Prefer beads when initialized: true DAG semantics with cycle detection.
if command -v bd >/dev/null 2>&1 && bd list --all --json >/dev/null 2>&1; then
  bd ready --label "$LABEL" --json --limit 200 2>/dev/null \
    | jq -c '.[] | select(.external_ref != null) | select(.external_ref | startswith("gh-")) | {number: (.external_ref | sub("^gh-"; "") | tonumber), title}' \
    | while IFS= read -r issue; do
        num=$(echo "$issue" | jq -r '.number')
        excluded "$num" && continue
        echo "$issue"
      done
  exit 0
fi

# Fallback: parse "## Blocked by" from issue bodies.
gh issue list --label "$LABEL" --state open --limit 200 --json number,title,body \
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
