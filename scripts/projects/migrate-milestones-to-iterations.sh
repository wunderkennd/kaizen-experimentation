#!/usr/bin/env bash
#
# migrate-milestones-to-iterations.sh
#   Onboard Milestone-tracked work onto a Projects-v2 board + Iteration field.
#
# Dry-run by default — pass --apply to mutate. Idempotent and re-runnable.
#
# Model (see docs/guides/projects-and-goals.md):
#   - Iterations are FORWARD-LOOKING cadence buckets and can't be created via API.
#     So we do NOT back-date one iteration per historical sprint. Instead:
#       * Active (open) work is added to the Project and assigned the current
#         Iteration, plus a machine-readable `sprint-N` mirror label.
#       * Closed historical sprints stay as their (closed) Milestones — the
#         archive — and are migrated only if you ask (--state all/closed).
#   - `sprint-N` labels remain the REST-visible sprint record that existing
#     orchestration tooling reads.
#
# Iteration data is read via GraphQL (gh project field-list does NOT return
# iteration titles/ids — only id|name|type).
#
# Requirements: gh CLI with `project` scope; jq.
#
# Usage (active-only, the default and recommended path):
#   ./scripts/projects/migrate-milestones-to-iterations.sh \
#       --owner wunderkennd --project 5 [--state open] [--iteration "Iteration 1"] [--apply]
#
# Flags:
#   --owner <login>        (required)
#   --project <number>     (required)
#   --state open|closed|all   issues to migrate per milestone   (default: open)
#   --milestone "TITLE"       only this milestone                (default: all)
#   --iteration "TITLE"       iteration assigned to migrated OPEN issues
#                             (default: the earliest current/upcoming iteration)
#   --apply                   actually mutate (default: dry-run)
#
set -euo pipefail

OWNER=""; PROJECT=""; STATE="open"; ONLY_MS=""; ITER_TITLE=""; APPLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --owner)     OWNER="$2"; shift 2 ;;
    --project)   PROJECT="$2"; shift 2 ;;
    --state)     STATE="$2"; shift 2 ;;
    --milestone) ONLY_MS="$2"; shift 2 ;;
    --iteration) ITER_TITLE="$2"; shift 2 ;;
    --apply)     APPLY=1; shift ;;
    -h|--help)   sed -n '2,40p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done
[ -n "$OWNER" ]   || { echo "ERROR: --owner required" >&2; exit 2; }
[ -n "$PROJECT" ] || { echo "ERROR: --project <number> required" >&2; exit 2; }
case "$STATE" in open|closed|all) ;; *) echo "ERROR: --state must be open|closed|all" >&2; exit 2 ;; esac

REPO="$OWNER/kaizen-experimentation"
run() { if [ "$APPLY" -eq 1 ]; then "$@"; else echo "DRY-RUN: $*"; fi; }

sprint_label() {  # "Sprint I.3: Multi-Cloud" -> "sprint-i.3"
  local title="$1" tok
  tok=$(printf '%s' "$title" | grep -oiE '[0-9I]+\.[0-9]+' | head -1 || true)
  if [ -n "$tok" ]; then
    printf 'sprint-%s' "$(printf '%s' "$tok" | tr '[:upper:]' '[:lower:]')"
  else
    printf 'sprint-%s' "$(printf '%s' "$title" | tr '[:upper:] ' '[:lower:]-' | tr -cd 'a-z0-9.-' | cut -c1-30)"
  fi
}

# --- Preflight ----------------------------------------------------------------
command -v gh >/dev/null || { echo "ERROR: gh CLI not found" >&2; exit 1; }
command -v jq >/dev/null || { echo "ERROR: jq not found" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "ERROR: gh not authenticated" >&2; exit 1; }

# --- Resolve Project + Iteration field via GraphQL ----------------------------
PROJECT_ID=$(gh project view "$PROJECT" --owner "$OWNER" --format json --jq '.id')
[ -n "$PROJECT_ID" ] || { echo "ERROR: could not resolve project id" >&2; exit 1; }

ITER_GQL=$(gh api graphql -f query='
  query($pid:ID!){ node(id:$pid){ ... on ProjectV2 {
    field(name:"Iteration"){ ... on ProjectV2IterationField {
      id configuration { iterations { id title startDate } } } } } } }' \
  -f pid="$PROJECT_ID" 2>/dev/null || echo '{}')

ITER_FIELD_ID=$(printf '%s' "$ITER_GQL" | jq -r '.data.node.field.id // empty')
# Choose target iteration id: explicit --iteration title, else earliest available.
if [ -n "$ITER_TITLE" ]; then
  ITER_ID=$(printf '%s' "$ITER_GQL" | jq -r --arg t "$ITER_TITLE" \
    '.data.node.field.configuration.iterations[]? | select(.title==$t) | .id' | head -1)
  ITER_PICK="$ITER_TITLE"
else
  ITER_ID=$(printf '%s'  "$ITER_GQL" | jq -r '.data.node.field.configuration.iterations[0]?.id // empty')
  ITER_PICK=$(printf '%s' "$ITER_GQL" | jq -r '.data.node.field.configuration.iterations[0]?.title // empty')
fi

echo "== Migration plan =="
echo "  Project:        #$PROJECT ($PROJECT_ID)"
echo "  Issue state:    $STATE"
echo "  Iteration field:${ITER_FIELD_ID:-<none>}"
if [ -n "$ITER_FIELD_ID" ] && [ -n "$ITER_ID" ]; then
  echo "  Assign open →   '$ITER_PICK' ($ITER_ID)"
else
  echo "  Assign open →   (no iteration available — open issues get label+board only)"
fi
echo

# --- Milestones ---------------------------------------------------------------
MS_JSON=$(gh api "repos/$REPO/milestones?state=all&per_page=100")
TITLES=$(printf '%s' "$MS_JSON" | jq -r --arg only "$ONLY_MS" \
  '.[] | select(($only=="") or (.title==$only)) | .title')

[ -n "$TITLES" ] || { echo "No matching milestones."; exit 0; }

printf '%s\n' "$TITLES" | while IFS= read -r title; do
  label=$(sprint_label "$title")
  echo "-- $title  (label=$label) --"
  run gh label create "$label" --repo "$REPO" --color "ededed" \
      --description "Sprint mirror of '$title'" 2>/dev/null || true

  gh issue list --repo "$REPO" --milestone "$title" --state "$STATE" \
      --limit 500 --json number,url,state \
      --jq '.[] | [.number, .url, .state] | @tsv' \
  | while IFS=$'\t' read -r num url istate; do
      echo "   #$num ($istate)"
      run gh issue edit "$num" --repo "$REPO" --add-label "$label"
      if [ "$APPLY" -eq 1 ]; then
        ITEM_ID=$(gh project item-add "$PROJECT" --owner "$OWNER" --url "$url" \
                  --format json --jq '.id' 2>/dev/null || true)
      else
        echo "   DRY-RUN: gh project item-add $PROJECT --owner $OWNER --url $url"
        ITEM_ID=""
      fi
      # Assign the current iteration to OPEN issues only.
      if [ "$istate" = "OPEN" ] && [ -n "$ITER_FIELD_ID" ] && [ -n "$ITER_ID" ]; then
        if [ -n "${ITEM_ID:-}" ]; then
          run gh project item-edit --id "$ITEM_ID" --project-id "$PROJECT_ID" \
              --field-id "$ITER_FIELD_ID" --iteration-id "$ITER_ID"
        else
          echo "   DRY-RUN: gh project item-edit --id <item> --field-id $ITER_FIELD_ID --iteration-id $ITER_ID"
        fi
      fi
    done
done

cat <<EOF

== NEXT ==
  - Verify open items landed on Project #$PROJECT with iteration '$ITER_PICK'.
  - Closed historical sprints remain as (closeable) Milestones — the archive.
  - After a transition sprint on both systems, drop Milestone reads from the
    justfile (see docs/guides/projects-and-goals.md → Migration from Milestones).
EOF
