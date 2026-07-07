#!/usr/bin/env bash
# Shared helpers for the executor-agnostic dispatch layer (harness H1, #679).
# Sourced by claims.sh / ready.sh / dispatch.sh — not executed directly.
#
# Claim state is carried on the GitHub Issue itself (externalized state, never
# executor session memory — proposal §7 R2): a `claimed` label plus marker
# comments. The LAST marker comment wins:
#   claim:          executor=<e> worker=<w> expires=<ISO8601Z>   (active lease)
#   claim-released: worker=<w>                                    (voluntary release)
#   claim-expired:  worker=<w> at=<ISO8601Z>                      (swept lease)

set -euo pipefail

CLAIM_LABEL="claimed"
HUMAN_GATE_LABEL="needs-human-input"
CLAIM_TTL_HOURS="${ORCH_CLAIM_TTL_HOURS:-24}"

now_iso() { date -u +%Y-%m-%dT%H:%M:%SZ; }

expiry_iso() {
  # GNU date first (Linux/CI), BSD date fallback (macOS operator machines).
  date -u -d "+${CLAIM_TTL_HOURS} hours" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null \
    || date -u -v "+${CLAIM_TTL_HOURS}H" +%Y-%m-%dT%H:%M:%SZ
}

ensure_claim_label() {
  gh label create "$CLAIM_LABEL" \
    --color "d93f0b" \
    --description "Leased to a harness worker (see latest 'claim:' comment)" \
    2>/dev/null || true
}

# All marker comments on an issue, oldest→newest, one per line.
claim_markers() {
  gh issue view "$1" --json comments \
    --jq '.comments[].body' 2>/dev/null \
    | grep -E '^(claim|claim-released|claim-expired):' || true
}

# The governing marker (last one). Empty when no claim history.
last_marker() { claim_markers "$1" | tail -1; }

marker_field() { # marker_field "<line>" <key>
  printf '%s\n' "$1" | grep -oE "${2}=[^ ]+" | head -1 | cut -d= -f2- || true
}

# Prints "worker expires" when the issue holds an UNEXPIRED active lease.
active_claim() {
  local line worker expires
  line=$(last_marker "$1")
  case "$line" in
    claim:*) ;;
    *) return 1 ;;
  esac
  worker=$(marker_field "$line" worker)
  expires=$(marker_field "$line" expires)
  [ -n "$expires" ] || return 1
  if [[ "$expires" > "$(now_iso)" ]]; then
    printf '%s %s\n' "$worker" "$expires"
    return 0
  fi
  return 1
}

has_claim_label() {
  gh issue view "$1" --json labels \
    --jq '[.labels[].name] | any(. == "'"$CLAIM_LABEL"'")' 2>/dev/null | grep -q true
}

# Space-separated issue numbers with an open PR that closes them.
in_flight_numbers() {
  gh pr list --state open --limit 200 \
    --json closingIssuesReferences \
    --jq '[.[].closingIssuesReferences[].number] | unique | join(" ")' 2>/dev/null || echo ""
}

# Space-separated open issue numbers currently carrying the claim label.
claimed_numbers() {
  gh issue list --label "$CLAIM_LABEL" --state open --limit 200 \
    --json number --jq '[.[].number] | join(" ")' 2>/dev/null || echo ""
}

# Space-separated open issue numbers carrying the operator-gate label: a human
# owes an action no machine lane can take (live validation, credentials, a
# settings toggle). Machine ready sets skip them — symmetrically across the
# native and body-parse paths, so READY_DRIFT keeps comparing like with like.
human_gated_numbers() {
  gh issue list --label "$HUMAN_GATE_LABEL" --state open --limit 200 \
    --json number --jq '[.[].number] | join(" ")' 2>/dev/null || echo ""
}

contains_num() { # contains_num "<space list>" <num>
  [ -n "$1" ] && printf ' %s ' "$1" | grep -q " $2 "
}
