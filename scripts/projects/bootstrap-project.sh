#!/usr/bin/env bash
#
# bootstrap-project.sh — Create the kaizen GitHub Project (v2) and its fields.
#
# Idempotent: re-running skips fields that already exist (matched by name).
# Dry-run by default — pass --apply to actually mutate.
#
# What it DOES (all via the Projects-v2 API):
#   - Create the Project (if missing).
#   - Create single-select fields: Status, Goal, Owner, Priority, Cluster.
#   - Create text field (ADR) and number field (Estimate).
#   - Create the ITERATION field, seeded with 3 × 14-day iterations from next Monday.
#   - Reconcile single-select option drift (e.g. GitHub's default Status options)
#     WHEN SAFE — i.e. when no project items use the field yet. updateProjectV2Field
#     replaces the whole option set and can orphan in-use values, so if items already
#     use the field the script WARNs instead of auto-replacing.
#
# The ONLY genuinely UI-only step is VIEWS (Board / Roadmap / By Agent) — the
# Projects-v2 API has no view-creation mutation. The script prints the view spec.
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
field_id()      { printf '%s' "$FIELDS_JSON" | jq -r --arg n "$1" '.fields[]? | select(.name==$n) | .id'; }

# Project node id (needed for GraphQL field create + usage checks).
PROJECT_ID=""
if [ "$PROJECT_NUMBER" != "<new>" ]; then
  PROJECT_ID="$(gh project view "$PROJECT_NUMBER" --owner "$OWNER" --format json --jq '.id' 2>/dev/null || true)"
fi

gql() { gh api graphql -f query="$1" "${@:2}"; }

# "A,B,C" -> {name:"A",color:GRAY,description:""},{name:"B",...}
opts_to_gql() {
  local IFS=',' parts=() o out=""
  read -ra parts <<<"$1"
  for o in "${parts[@]}"; do out+="{name:\"$o\",color:GRAY,description:\"\"},"; done
  printf '%s' "${out%,}"
}

# Is a single-select field in use? echoes no|yes|unknown.
# "no" = safe to replace options; "yes"/"unknown" = do NOT auto-replace.
field_in_use() {
  local name="$1" total used
  [ -z "$PROJECT_ID" ] && { echo "unknown"; return; }
  total=$(gh project item-list "$PROJECT_NUMBER" --owner "$OWNER" --format json --jq '.items | length' 2>/dev/null || echo "?")
  [ "$total" = "?" ] && { echo "unknown"; return; }
  [ "$total" -eq 0 ] && { echo "no"; return; }
  used=$(gql 'query($pid:ID!,$fn:String!){node(id:$pid){... on ProjectV2{items(first:100){nodes{fieldValueByName(name:$fn){__typename}}}}}}' \
           -f pid="$PROJECT_ID" -f fn="$name" \
           --jq '[.data.node.items.nodes[]? | select(.fieldValueByName!=null)] | length' 2>/dev/null || echo "?")
  [ "$used" = "?" ] && { echo "unknown"; return; }
  if   [ "$used" -gt 0 ];   then echo "yes"
  elif [ "$total" -gt 100 ]; then echo "unknown"   # only sampled 100 — can't be sure
  else echo "no"; fi
}

# Portable date helpers (GNU then BSD then today-fallback).
next_monday() {
  date -d "next monday" +%Y-%m-%d 2>/dev/null && return
  date -v+Mon +%Y-%m-%d 2>/dev/null && return
  date +%Y-%m-%d
}
add_days() {  # $1=YYYY-MM-DD $2=N
  date -d "$1 +$2 days" +%Y-%m-%d 2>/dev/null && return
  date -j -v+"$2"d -f "%Y-%m-%d" "$1" +%Y-%m-%d 2>/dev/null && return
  printf '%s' "$1"
}

create_single_select() {
  local name="$1" opts="$2" want_set have_raw have_set usage fid gqlopts
  # Compare as a SET (order-insensitive): a mere reorder isn't worth a reconcile.
  want_set="$(printf '%s' "$opts" | tr ',' '\n' | sort | paste -sd',' -)"
  if has_field "$name"; then
    have_raw="$(field_options "$name")"
    have_set="$(printf '%s' "$have_raw" | tr '|' '\n' | sort | paste -sd',' -)"
    if [ "$have_set" = "$want_set" ]; then
      echo "  field '$name' exists with the right option set — skip"; return
    fi
    # Option SET drift. Auto-reconcile only if no items use the field (else orphan risk).
    usage="$(field_in_use "$name")"
    echo "  field '$name' option set differs (usage=$usage)"
    echo "    have: ${have_raw:-<none>}"
    echo "    want: $opts"
    if [ "$usage" = "no" ]; then
      if [ "$APPLY" -eq 1 ]; then
        fid="$(field_id "$name")"; gqlopts="$(opts_to_gql "$opts")"
        gql "mutation{updateProjectV2Field(input:{fieldId:\"$fid\",singleSelectOptions:[$gqlopts]}){clientMutationId}}" >/dev/null \
          && echo "    reconciled '$name' options via API (no items used it)"
      else
        echo "    DRY-RUN: would reconcile '$name' options via updateProjectV2Field (safe — unused)"
      fi
    else
      echo "    WARN: usage=$usage — NOT auto-reconciling (replace can orphan in-use values)."
      echo "          Reconcile deliberately in the UI or via updateProjectV2Field."
    fi
    return
  fi
  run gh project field-create "$PROJECT_NUMBER" --owner "$OWNER" \
    --name "$name" --data-type SINGLE_SELECT --single-select-options "$opts"
  if [ "$APPLY" -eq 1 ]; then echo "  created single-select '$name'"; fi
}
create_field() {
  local name="$1" dtype="$2"
  if has_field "$name"; then echo "  field '$name' exists — skip"; return; fi
  run gh project field-create "$PROJECT_NUMBER" --owner "$OWNER" \
    --name "$name" --data-type "$dtype"
  if [ "$APPLY" -eq 1 ]; then echo "  created $dtype '$name'"; fi
}
create_iteration_field() {
  if has_field "Iteration"; then echo "  field 'Iteration' exists — skip"; return; fi
  local d1 d2 d3 iters
  d1="$(next_monday)"; d2="$(add_days "$d1" 14)"; d3="$(add_days "$d1" 28)"
  iters="{title:\"Iteration 1\",startDate:\"$d1\",duration:14},"
  iters+="{title:\"Iteration 2\",startDate:\"$d2\",duration:14},"
  iters+="{title:\"Iteration 3\",startDate:\"$d3\",duration:14}"
  if [ "$APPLY" -eq 1 ]; then
    [ -n "$PROJECT_ID" ] || { echo "  WARN: no PROJECT_ID — cannot create Iteration field"; return; }
    gql "mutation{createProjectV2Field(input:{projectId:\"$PROJECT_ID\",dataType:ITERATION,name:\"Iteration\",iterationConfiguration:{startDate:\"$d1\",duration:14,iterations:[$iters]}}){projectV2Field{... on ProjectV2IterationField{id}}}}" >/dev/null \
      && echo "  created Iteration field (3 × 14-day iterations from $d1)"
  else
    echo "  DRY-RUN: create Iteration field — 3 × 14-day iterations from $d1"
  fi
}

# --- Create fields ------------------------------------------------------------
echo "-- Single-select fields --"
create_single_select "Status"   "Backlog,Ready,In Progress,In Review,Blocked,Done"
create_single_select "Priority" "P0,P1,P2,P3,P4"
create_single_select "Cluster"  "cluster-a,cluster-b,cluster-c,cluster-d,cluster-e,cluster-f,cluster-g"
create_single_select "Owner"    "agent-1,agent-2,agent-3,agent-4,agent-5,agent-6,agent-7,infra-1,infra-2,infra-3,infra-4,infra-5"
# Goal mirrors parent-issue titles; seed with the 8 current goals (extend as needed).
create_single_select "Goal"     "ADR-026 Custom Metrics,ADR-027 TOST,ADR-028 Shadow Inference,ADR-029 Calibration,ADR-030 Shadow Mode,Infrastructure GA,QoE Observability GA,Palette Standardization,ADR-031 ConnectRPC"

echo "-- Text / number fields --"
create_field "ADR"      TEXT
create_field "Estimate" NUMBER

echo "-- Iteration field --"
create_iteration_field

# --- Remaining steps ----------------------------------------------------------
cat <<EOF

== DONE (API portion) ==
Project: https://github.com/users/$OWNER/projects/$PROJECT_NUMBER

Automated above (when --apply): all fields incl. Iteration (3 × 14-day iterations),
plus Status option reconciliation when no items use the field yet.

== REMAINING STEP (the only genuinely UI-only piece) ==

VIEWS  (Projects-v2 has no view-creation API — create in the UI):
   a. "Board"    — Board   — Group by: Status
   b. "Roadmap"  — Roadmap — Group by: Goal — Date field: Iteration
   c. "By Agent" — Table   — Group by: Owner

Notes:
 - If Status options drifted but items already use the field, the script WARNs
   instead of replacing (replacing can orphan in-use values) — reconcile manually.
 - Iteration iterations are named Iteration 1/2/3; rename in the UI to match sprints.

Then run:
   ./scripts/projects/migrate-milestones-to-iterations.sh --owner $OWNER --project $PROJECT_NUMBER
EOF
