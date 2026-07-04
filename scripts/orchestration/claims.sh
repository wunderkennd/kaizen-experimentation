#!/usr/bin/env bash
# Issue claim protocol (harness H1, #679). Subcommands:
#
#   claims.sh take <issue> <executor> <worker-id> [ttl-hours]
#       Take a lease. Exit 0 = claimed; exit 3 = already claimed (someone
#       else holds an unexpired lease, or we lost the post-claim race).
#   claims.sh release <issue> <worker-id>
#       Voluntarily release (adapter failed, worker done without a PR).
#   claims.sh sweep
#       Expire stale leases repo-wide: claimed label + expired lease + no
#       open closing PR → claim-expired comment + label removed. A claimed
#       issue whose lease is superseded by an open closing PR just loses
#       the label (the PR is the stronger in-flight signal).
#   claims.sh status <issue>
#       Human-readable claim state; exit 0 active, 1 not claimed.
#
# Atomicity note: GitHub offers no CAS on labels/comments, so `take` is
# check → label → comment → re-check. Two dispatchers racing both post
# claims; the one whose claim comment sorts FIRST wins, the loser posts a
# release and exits 3. The window is a few seconds; the loser always
# detects it. This is deliberately dumb — any executor with `gh` can play.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

take() {
  local issue="$1" executor="$2" worker="$3" ttl="${4:-$CLAIM_TTL_HOURS}"
  CLAIM_TTL_HOURS="$ttl"

  local held
  if held=$(active_claim "$issue"); then
    echo "already claimed: issue #$issue held by ${held% *} until ${held#* }" >&2
    return 3
  fi

  ensure_claim_label
  gh issue edit "$issue" --add-label "$CLAIM_LABEL" >/dev/null
  gh issue comment "$issue" \
    --body "claim: executor=$executor worker=$worker expires=$(expiry_iso)" >/dev/null

  # Post-claim race check: if another unexpired claim precedes ours in the
  # marker stream (after the last release/expire), we lost — back off.
  local stream first_claim first_worker
  stream=$(claim_markers "$issue")
  first_claim=$(printf '%s\n' "$stream" \
    | awk '/^(claim-released|claim-expired):/{buf=""} /^claim:/{if(buf=="")buf=$0} END{print buf}')
  first_worker=$(marker_field "$first_claim" worker)
  if [ -n "$first_worker" ] && [ "$first_worker" != "$worker" ]; then
    local fexp
    fexp=$(marker_field "$first_claim" expires)
    if [ -n "$fexp" ] && [[ "$fexp" > "$(now_iso)" ]]; then
      gh issue comment "$issue" --body "claim-released: worker=$worker" >/dev/null
      echo "already claimed: lost race to $first_worker on issue #$issue" >&2
      return 3
    fi
  fi
  echo "claimed: issue #$issue by $worker (ttl ${ttl}h)"
}

release() {
  local issue="$1" worker="$2"
  gh issue comment "$issue" --body "claim-released: worker=$worker" >/dev/null
  gh issue edit "$issue" --remove-label "$CLAIM_LABEL" >/dev/null 2>&1 || true
  echo "released: issue #$issue by $worker"
}

sweep() {
  local in_flight nums n line worker expires
  in_flight=$(in_flight_numbers)
  nums=$(claimed_numbers)
  [ -n "$nums" ] || { echo "sweep: no claimed issues"; return 0; }
  for n in $nums; do
    if contains_num "$in_flight" "$n"; then
      # Open closing PR supersedes the lease — drop the label quietly.
      gh issue edit "$n" --remove-label "$CLAIM_LABEL" >/dev/null 2>&1 || true
      echo "sweep: #$n superseded by open PR — label removed"
      continue
    fi
    if active_claim "$n" >/dev/null; then
      echo "sweep: #$n lease still live"
      continue
    fi
    line=$(last_marker "$n")
    worker=$(marker_field "$line" worker)
    case "$line" in
      claim:*)
        gh issue comment "$n" \
          --body "claim-expired: worker=${worker:-unknown} at=$(now_iso)" >/dev/null
        ;;
    esac
    gh issue edit "$n" --remove-label "$CLAIM_LABEL" >/dev/null 2>&1 || true
    echo "sweep: #$n stale lease cleared (worker=${worker:-none})"
  done
}

status() {
  local issue="$1" held
  if held=$(active_claim "$issue"); then
    echo "issue #$issue: claimed by ${held% *} until ${held#* }"
    return 0
  fi
  echo "issue #$issue: not claimed"
  return 1
}

cmd="${1:-}"; shift || true
case "$cmd" in
  take)    take "$@" ;;
  release) release "$@" ;;
  sweep)   sweep "$@" ;;
  status)  status "$@" ;;
  *) echo "usage: claims.sh {take <issue> <executor> <worker> [ttl-h] | release <issue> <worker> | sweep | status <issue>}" >&2
     exit 2 ;;
esac
