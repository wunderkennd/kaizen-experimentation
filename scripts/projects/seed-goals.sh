#!/usr/bin/env bash
#
# seed-goals.sh — File Goal issues (label:goal) and add them to the Project.
#
# A Goal = an outcome with ONE success metric, parent of native sub-issues.
# One per ADR or named initiative (see docs/guides/projects-and-goals.md).
#
# Dry-run by default — pass --apply to mutate. Idempotent: skips a goal whose
# exact title already exists as an issue (open or closed — query uses --state all).
#
# For each goal it: creates the issue (label:goal), adds it to the Project, and
# links its child issues as NATIVE sub-issues (REST sub_issues API) so the
# parent progress bar tracks closure.
#
# Requirements: gh CLI with `project` scope; jq.
#
# Usage:
#   ./scripts/projects/seed-goals.sh --owner wunderkennd --project 5 [--filter k1,k2] [--apply]
#
# Filter keys: infra connectrpc adr-026 adr-027 adr-028 adr-029 adr-030 qoe palette
#
set -euo pipefail

OWNER=""; PROJECT=""; FILTER=""; APPLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --owner)   OWNER="$2"; shift 2 ;;
    --project) PROJECT="$2"; shift 2 ;;
    --filter)  FILTER="$2"; shift 2 ;;
    --apply)   APPLY=1; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done
[ -n "$OWNER" ]   || { echo "ERROR: --owner required" >&2; exit 2; }
[ -n "$PROJECT" ] || { echo "ERROR: --project <number> required" >&2; exit 2; }

REPO="$OWNER/kaizen-experimentation"
command -v gh >/dev/null || { echo "ERROR: gh CLI not found" >&2; exit 1; }
command -v jq >/dev/null || { echo "ERROR: jq not found" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "ERROR: gh not authenticated" >&2; exit 1; }
run() { if [ "$APPLY" -eq 1 ]; then "$@"; else echo "DRY-RUN: $*"; fi; }

in_filter() { [ -z "$FILTER" ] && return 0; printf ',%s,' "$FILTER" | grep -q ",$1,"; }

# Ensure the `goal` label exists.
run gh label create goal --repo "$REPO" --color "5319e7" \
    --description "Outcome with a success metric; parent of sub-issues" 2>/dev/null || true

# Cache existing goal issue titles for idempotency.
EXISTING=$(gh issue list --repo "$REPO" --label goal --state all --limit 200 \
           --json title --jq '.[].title' 2>/dev/null || true)

seed_goal() {
  local key="$1" title="$2" adr="$3" outcome="$4" metric="$5" children="$6"
  in_filter "$key" || return 0
  echo "== [$key] $title =="
  if printf '%s\n' "$EXISTING" | grep -qxF "$title"; then
    echo "  exists — skip"; return 0
  fi

  local kids_md="_none yet — add child issues as work is filed_"
  if [ -n "$children" ]; then
    kids_md=""
    IFS=',' read -ra arr <<<"$children"
    for n in "${arr[@]}"; do kids_md+="- [ ] #$n"$'\n'; done
  fi

  local body
  body=$(cat <<EOF
## Outcome
$outcome

## Success metric
$metric

## Source
- **ADR:** $adr

## Child issues
$kids_md

> Filed by scripts/projects/seed-goals.sh. Child issues are linked as native
> sub-issues; the progress bar above tracks their closure.
> See docs/guides/projects-and-goals.md.
EOF
)

  if [ "$APPLY" -eq 1 ]; then
    local url num
    url=$(gh issue create --repo "$REPO" --title "$title" --label goal --body "$body")
    echo "  created: $url"
    num="${url##*/}"
    gh project item-add "$PROJECT" --owner "$OWNER" --url "$url" >/dev/null && echo "  added to project #$PROJECT"
    if [ -n "$children" ]; then
      IFS=',' read -ra arr <<<"$children"
      for n in "${arr[@]}"; do
        local cid
        cid=$(gh api "repos/$REPO/issues/$n" --jq '.id' 2>/dev/null || true)
        if [ -n "$cid" ]; then
          gh api -X POST "repos/$REPO/issues/$num/sub_issues" -F sub_issue_id="$cid" >/dev/null 2>&1 \
            && echo "  linked sub-issue #$n" || echo "  WARN: could not link #$n (may already be linked)"
        fi
      done
    fi
  else
    echo "  DRY-RUN: gh issue create --title \"$title\" --label goal"
    [ -n "$children" ] && echo "  DRY-RUN: link sub-issues: $children"
    echo "  DRY-RUN: add to project #$PROJECT"
  fi
}

#         key          title                                                                          ADR                outcome                                                                                          metric                                                                                            children
seed_goal infra        "Goal: Infrastructure GA (Pulumi/AWS) — all 9 Kaizen services deployed"         "none (initiative)" "All 9 Kaizen services deployed on AWS via Pulumi with observability and green mock suites."        "9/9 services deployed; fullstack mock suite green; observability (logging/metrics/tracing) wired." "496,498,500,501,502"
seed_goal connectrpc   "Goal: ADR-031 ConnectRPC Pilot — unified Connect transport for M1 assignment"  "ADR-031"           "M1 assignment RPCs served over ConnectRPC, retiring hand-rolled JSON shims."                      "GetAssignment + remaining unary + StreamConfigUpdates over Connect; JSON shim retired; pilot meets success/kill criteria." "641,642,643,644,645"
seed_goal adr-026      "Goal: ADR-026 Custom Metrics Layer — operators self-serve custom metrics"      "ADR-026"           "Operators define composite/derived/joined metrics beyond the six built-in types without engineering." "3 Tier-1 types (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT) GA; <5% definition-validation error over 30 days." ""
seed_goal adr-027      "Goal: ADR-027 TOST Equivalence Testing — prove equivalence for migrations"     "ADR-027"           "Teams prove statistical equivalence (not just non-difference) for infra migrations and refactors."  "TOST exposed in M4a/M5/M6; golden files match R TOSTER (tsum_TOST) to 6 decimal places."           ""
seed_goal adr-028      "Goal: ADR-028 M4b Shadow Inference — safe bandit policy promotion"             "ADR-028"           "Bandit policies promote via a dedicated shadow core with column-family isolation, no prod impact."  "Shadow core promotes policies with zero prod-traffic exposure regressions."                        ""
seed_goal adr-029      "Goal: ADR-029 Cross-Modal Score Calibration — unified NEV scale"               "ADR-029"           "Heterogeneous slates (video, manga, commerce) score on one calibrated NEV scale."                  "Unified NEV scale across >=3 modalities; calibration error within target band."                   ""
seed_goal adr-030      "Goal: ADR-030 Shadow Experiment Mode — candidate variants on prod traffic"     "ADR-030"           "Experiments run candidate variants on production traffic without exposing users."                   "Candidate variants run on prod traffic with 0 user-facing exposure incidents."                    ""
seed_goal qoe          "Goal: QoE Observability GA — server-side QoE aggregation"                      "none (initiative)" "EBVS is first-class on PlaybackMetrics and HeartbeatSessionizer delivers server-side QoE aggregation." "Server-side QoE aggregation live; EBVS first-class on PlaybackMetrics; heartbeat sessionization in prod." ""
seed_goal palette      "Goal: Palette / M6 Design-System Standardization"                             "none (initiative)" "Search, empty states, filter-clearing, and CopyButton are standardized across M6 with a11y passing." "Standardized patterns across M6 surfaces; accessibility audit passes."                             ""

echo
echo "== DONE (filter='${FILTER:-all}', apply=$APPLY) =="
echo "Goals board: https://github.com/users/$OWNER/projects/$PROJECT"
