#!/usr/bin/env bash
# evening_dispatch.sh — H4 Phase A (#716, plan 2026-07-06)
#
#   evening_dispatch.sh [--live] [cohort-label ...]
#
# Nightly dispatcher core. SHADOW by default: computes the ready set per
# sprint cohort (H2 native _ready via ready.sh), dedupes across cohorts,
# applies the cap, and reports what it WOULD dispatch — claims nothing,
# launches nothing. --live hands each selected issue to dispatch.sh
# (H1: claim → prompt render → adapter), executor claude-workflow.
# Live is additionally double-gated in evening-dispatcher.yml (plan L4).
#
# Env knobs:
#   DISPATCH_CAP   max issues per run across all cohorts (default 3; plan L5)
#   DISPATCH_BIN   dispatch entrypoint for --live (default: sibling
#                  dispatch.sh; overridable for offline tests)
#
# Exit codes: 0 clean (shadow always, live with no adapter failures);
#             1 one or more live dispatches failed.
set -uo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

LIVE=0
COHORTS=()
for a in "$@"; do
  case "$a" in
    --live) LIVE=1 ;;
    *) COHORTS+=("$a") ;;
  esac
done

CAP="${DISPATCH_CAP:-3}"
DISPATCH_BIN="${DISPATCH_BIN:-$HERE/dispatch.sh}"
MODE=$([ "$LIVE" -eq 1 ] && echo LIVE || echo shadow)

if [ ${#COHORTS[@]} -eq 0 ]; then
  while IFS= read -r l; do [ -n "$l" ] && COHORTS+=("$l"); done < <(
    gh issue list --state open --limit 200 --json labels \
      --jq '[.[].labels[].name | select(startswith("sprint-"))] | unique | .[]' 2>/dev/null || true)
fi

if [ ${#COHORTS[@]} -eq 0 ]; then
  echo "evening-dispatch ($MODE): no open sprint-* cohorts — nothing to consider."
  exit 0
fi

# Ready candidates across cohorts; dedupe by number; ascending order (L5).
# Readiness comes ONLY from ready.sh (L6) — native edges, claims, in-flight.
CAND=$(for L in "${COHORTS[@]}"; do bash "$HERE/ready.sh" "$L" 2>/dev/null || true; done \
  | jq -s -c 'unique_by(.number) | sort_by(.number) | .[]' 2>/dev/null || true)

TOTAL=$(printf '%s\n' "$CAND" | grep -c '^{' || true)
SELECTED=$(printf '%s\n' "$CAND" | grep '^{' | head -n "$CAP" || true)
PICKED=$(printf '%s\n' "$SELECTED" | grep -c '^{' || true)
OVERFLOW=$((TOTAL - PICKED))

REPORT="## Evening dispatch ($MODE) — $(date -u +%Y-%m-%dT%H:%M:%SZ)
cohorts: ${COHORTS[*]} · ready: $TOTAL · cap: $CAP · selected: $PICKED"

emit() { printf '%s\n' "$1"; [ -n "${GITHUB_STEP_SUMMARY:-}" ] && printf '%s\n' "$1" >>"$GITHUB_STEP_SUMMARY" || true; }

emit "$REPORT"

DISPATCHED=0; ALREADY=0; FAILED=0
while IFS= read -r row; do
  [ -n "$row" ] || continue
  n=$(jq -r '.number' <<<"$row")
  t=$(jq -r '.title' <<<"$row")
  if [ "$LIVE" -eq 0 ]; then
    emit "- would dispatch #$n — $t"
    continue
  fi
  if bash "$DISPATCH_BIN" "$n" claude-workflow; then
    DISPATCHED=$((DISPATCHED + 1))
    emit "- dispatched #$n — $t"
  else
    rc=$?
    if [ "$rc" -eq 3 ]; then
      ALREADY=$((ALREADY + 1))
      emit "- #$n already claimed — skipped"
    else
      FAILED=$((FAILED + 1))
      emit "- #$n FAILED to dispatch (rc=$rc)"
    fi
  fi
done <<<"$SELECTED"

[ "$OVERFLOW" -gt 0 ] && emit "- ($OVERFLOW more ready beyond the cap)"
if [ "$LIVE" -eq 1 ]; then
  emit "summary: dispatched=$DISPATCHED already-claimed=$ALREADY failed=$FAILED"
fi
[ "$FAILED" -eq 0 ]
