#!/usr/bin/env bash
#
# bootstrap-project.sh — Create the kaizen GitHub Project (v2) and its fields.
#
# Idempotent: re-running skips fields that already exist (matched by name).
# Dry-run by default — pass --apply to actually mutate.
#
# What it DOES (GitHub Projects v2 API supports these):
#   - Create the Project (if missing).
#   - Create single-select fields: Status, Goal, Owner, Priority, Cluster.
#   - Create text field: ADR.
#   - Create number field: Estimate.
#
# What it CANNOT do (GitHub API does not expose these — do them in the UI):
#   - Create the ITERATION field (Projects v2 API cannot create iteration fields).
#   - Create saved VIEWS (Board / Roadmap / By Agent) with grouping.
# The script prints exact UI instructions for both at the end.
#
# Requirements: gh CLI authenticated with the `project` scope:
#   gh auth refresh -s project,read:project
#
# Usage:
#   ./scripts/projects/bootstrap-project.sh --owner wunderkennd [--title "..."] [--apply]
#
set -euo pipefail

OWNER=""
TITLE="Kaizen Experimentation"
APPLY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --owner) OWNER="$2"; shift 2 ;;
    --title) TITLE="$2"; shift 2 ;;
    --apply) APPLY=1; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [ -z "$OWNER" ]; then
  echo "ERROR: --owner <org-or-user> is required (e.g. --owner wunderkennd)" >&2
  exit 2
fi

run() {
  if [ "$APPLY" -eq 1 ]; then
    "$@"
  else
    echo "DRY-RUN: $*"
  fi
}

# --- Preflight ----------------------------------------------------------------
command -v gh >/dev/null || { echo "ERROR: gh CLI not found" >&2; exit 1; }
command -v jq >/dev/null || { echo "ERROR: jq not found" >&2; exit 1; }
if ! gh auth status >/dev/null 2>&1; then
  echo "ERROR: gh not authenticated. Run: gh auth login" >&2; exit 1
fi
if ! gh auth status 2>&1 | grep -qiE "project"; then
  echo "WARN: token may lack the 'project' scope. If field creation fails, run:" >&2
  echo "      gh auth refresh -s project,read:project" >&2
fi

echo "== Bootstrapping Project '$TITLE' for owner '$OWNER' (apply=$APPLY) =="

# --- Find or create the Project ----------------------------------------------
PROJECT_NUMBER=$(gh project list --owner "$OWNER" --format json \
  --jq ".projects[] | select(.title == \"$TITLE\") | .number" 2>/dev/null | head -1 || true)

if [ -n "$PROJECT_NUMBER" ]; then
  echo "Project exists: #$PROJECT_NUMBER"
else
  if [ "$APPLY" -eq 1 ]; then
    PROJECT_NUMBER=$(gh project create --owner "$OWNER" --title "$TITLE" \
      --format json --jq '.number')
    echo "Created Project #$PROJECT_NUMBER"
  else
    echo "DRY-RUN: gh project create --owner $OWNER --title \"$TITLE\""
    PROJECT_NUMBER="<new>"
  fi
fi

# --- Field helpers ------------------------------------------------------------
# Cache the full field JSON so we can compare single-select options, not just names.
# (GitHub auto-creates a default "Status" field with Todo|In Progress|Done — a
# name-only check would silently skip it and leave the wrong options.)
FIELDS_JSON='{"fields":[]}'
if [ "$PROJECT_NUMBER" != "<new>" ]; then
  FIELDS_JSON="$(gh project field-list "$PROJECT_NUMBER" --owner "$OWNER" --format json 2>/dev/null || echo '{"fields":[]}')"
fi

has_field()     { printf '%s' "$FIELDS_JSON" | jq -e --arg n "$1" '.fields[]? | select(.name==$n)' >/dev/null 2>&1; }
field_options() { printf '%s' "$FIELDS_JSON" | jq -r --arg n "$1" '.fields[]? | select(.name==$n) | [.options[]?.name] | join("|")'; }

create_single_select() {
  local name="$1" opts="$2" want have
  want="$(printf '%s' "$opts" | tr ',' '|')"
  if has_field "$name"; then
    have="$(field_options "$name")"
    if [ "$have" = "$want" ]; then
      echo "  field '$name' exists with correct options — skip"
    else
      echo "  WARN: field '$name' exists but options differ — reconcile in the UI:"
      echo "        have: ${have:-<none>}"
      echo "        want: $want"
    fi
    return
  fi
  run gh project field-create "$PROJECT_NUMBER" --owner "$OWNER" \
    --name "$name" --data-type SINGLE_SELECT --single-select-options "$opts"
  [ "$APPLY" -eq 1 ] && echo "  created single-select '$name'"
}
create_field() {
  local name="$1" dtype="$2"
  if has_field "$name"; then echo "  field '$name' exists — skip"; return; fi
  run gh project field-create "$PROJECT_NUMBER" --owner "$OWNER" \
    --name "$name" --data-type "$dtype"
  [ "$APPLY" -eq 1 ] && echo "  created $dtype '$name'"
}

# --- Create fields ------------------------------------------------------------
echo "-- Single-select fields --"
create_single_select "Status"   "Backlog,Ready,In Progress,In Review,Blocked,Done"
create_single_select "Priority" "P0,P1,P2,P3,P4"
create_single_select "Cluster"  "cluster-a,cluster-b,cluster-c,cluster-d,cluster-e,cluster-f,cluster-g"
create_single_select "Owner"    "agent-1,agent-2,agent-3,agent-4,agent-5,agent-6,agent-7,infra-1,infra-2,infra-3,infra-4,infra-5"
# Goal mirrors parent-issue titles; seed with the 8 current goals (extend as needed).
create_single_select "Goal"     "ADR-026 Custom Metrics,ADR-027 TOST,ADR-028 Shadow Inference,ADR-029 Calibration,ADR-030 Shadow Mode,Infrastructure GA,QoE Observability GA,Palette Standardization"

echo "-- Text / number fields --"
create_field "ADR"      TEXT
create_field "Estimate" NUMBER

# --- Manual steps the API cannot do ------------------------------------------
cat <<EOF

== DONE (API portion) ==
Project: https://github.com/users/$OWNER/projects/$PROJECT_NUMBER

== MANUAL UI STEPS (GitHub API cannot create these) ==

0. STATUS OPTIONS  (Project → ⚙ Settings → Status field)
   - GitHub seeds Status with Todo|In Progress|Done. Reconcile to:
     Backlog, Ready, In Progress, In Review, Blocked, Done
   - (Single-select options cannot be reliably set via the API; see WARN above.)

1. ITERATION FIELD  (Project → ⚙ Settings → + New field → Iteration)
   - Name:      Iteration
   - Duration:  2 weeks
   - Then add iterations named to match sprints: "Sprint 5.6", "Sprint I.2", ...
   - (The migrate-milestones-to-iterations.sh script will tell you which to add,
      derived from existing Milestone due dates.)

2. VIEWS  (Project → + New view)
   a. "Board"     — Layout: Board    — Group by: Status
   b. "Roadmap"   — Layout: Roadmap  — Group by: Goal    — Date field: Iteration
   c. "By Agent"  — Layout: Table    — Group by: Owner

After the Iteration field exists, run:
   ./scripts/projects/migrate-milestones-to-iterations.sh --owner $OWNER --project $PROJECT_NUMBER
EOF
