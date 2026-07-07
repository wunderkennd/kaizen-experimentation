#!/usr/bin/env bash
# Offline behavior tests for the H1 dispatch layer (#679). Stubs `gh` with a
# filesystem-backed fake and the executor adapter with a recorder, then drives
# claims.sh / ready.sh / dispatch.sh end-to-end. No network, no real gh.
#
# Covered acceptance criteria (#679):
#   A. double dispatch → exactly one worker; second exits 3 "already claimed"
#   B. stale lease sweeps clean and the issue becomes dispatchable again
#   C. re-dispatch after a progress-branch comment renders a RESUME prompt
#   D. ready.sh excludes claimed, in-flight, and operator-gated issues
#   E. adapter failure releases the claim
#
# Run: bash scripts/orchestration/test_dispatch.sh   (or `just test-orchestration`)

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

export GH_STATE="$WORK/state"
mkdir -p "$GH_STATE"
STUBS="$WORK/bin"
mkdir -p "$STUBS"

# ---------- filesystem-backed gh stub ----------
cat > "$STUBS/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
S="$GH_STATE"
die() { echo "gh-stub: unhandled: $*" >&2; exit 97; }

labels_of() { cat "$S/issues/$1/labels" 2>/dev/null || true; }
comments_of() { # bodies in creation order
  local d="$S/issues/$1/comments"
  [ -d "$d" ] && for f in $(ls "$d" | sort -n); do cat "$d/$f"; echo; done
}

case "$1 $2" in
  "label create")
    touch "$S/label_created_$3"; exit 0 ;;
  "repo view")
    echo "test/kaizen"; exit 0 ;;
  "pr list")
    # emit space-joined closing numbers per lib.in_flight_numbers jq
    cat "$S/prs_closing" 2>/dev/null || echo ""; exit 0 ;;
  "issue list")
    shift 2
    label=""; jsonf=""; jqx=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --label) label="$2"; shift 2 ;;
        --json)  jsonf="$2"; shift 2 ;;
        --jq)    jqx="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    nums=""
    for d in "$S"/issues/*/; do
      n=$(basename "$d")
      [ "$(cat "$d/state" 2>/dev/null || echo OPEN)" = "OPEN" ] || continue
      grep -qx "$label" "$d/labels" 2>/dev/null && nums="$nums $n"
    done
    nums=$(echo "$nums" | xargs -n1 2>/dev/null | sort -n | xargs || true)
    if [ "$jsonf" = "number" ]; then
      # claims.sh claimed_numbers path: join numbers with spaces
      echo "$nums"; exit 0
    fi
    # ready.sh fallback path: full JSON array (script pipes to real jq)
    printf '['
    first=1
    for n in $nums; do
      [ $first -eq 0 ] && printf ','
      first=0
      title=$(cat "$S/issues/$n/title" 2>/dev/null || echo "issue $n")
      body=$(cat "$S/issues/$n/body" 2>/dev/null || echo "")
      jq -cn --arg t "$title" --arg b "$body" --argjson n "$n" \
        '{number:$n,title:$t,body:$b}'
    done
    printf ']\n'; exit 0 ;;
  "issue view")
    n="$3"; shift 3
    json=""; q=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --json) json="$2"; shift 2 ;;
        --jq|-q) q="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    case "$json" in
      comments) comments_of "$n"; exit 0 ;;
      labels)
        if labels_of "$n" | grep -qx claimed; then echo true; else echo false; fi
        exit 0 ;;
      title) cat "$S/issues/$n/title" 2>/dev/null || echo "issue $n"; exit 0 ;;
      body)  cat "$S/issues/$n/body" 2>/dev/null || echo ""; exit 0 ;;
      state) cat "$S/issues/$n/state" 2>/dev/null || echo "OPEN"; exit 0 ;;
      *) die "issue view --json $json" ;;
    esac ;;
  "issue edit")
    n="$3"; shift 3
    mkdir -p "$S/issues/$n"
    while [ $# -gt 0 ]; do
      case "$1" in
        --add-label)
          grep -qx "$2" "$S/issues/$n/labels" 2>/dev/null || echo "$2" >> "$S/issues/$n/labels"
          shift 2 ;;
        --remove-label)
          [ -f "$S/issues/$n/labels" ] && grep -vx "$2" "$S/issues/$n/labels" > "$S/issues/$n/labels.tmp" \
            && mv "$S/issues/$n/labels.tmp" "$S/issues/$n/labels" || true
          shift 2 ;;
        *) shift ;;
      esac
    done
    exit 0 ;;
  "issue comment")
    n="$3"; shift 3
    mkdir -p "$S/issues/$n/comments"
    seq=$(( $(ls "$S/issues/$n/comments" 2>/dev/null | wc -l) + 1 ))
    body=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --body) body="$2"; shift 2 ;;
        --body-file) [ "$2" = "-" ] && body="$(cat)"; shift 2 ;;
        *) shift ;;
      esac
    done
    printf '%s' "$body" > "$S/issues/$n/comments/$seq"
    exit 0 ;;
esac
die "$@"
STUB
chmod +x "$STUBS/gh"

# ---------- recording adapter ----------
ADAPTERS="$WORK/adapters"
mkdir -p "$ADAPTERS"
cat > "$ADAPTERS/stubexec.sh" <<'A'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$GH_STATE/adapter"
echo "$1" >> "$GH_STATE/adapter/calls"
cat > "$GH_STATE/adapter/last_prompt_$1"
A
cat > "$ADAPTERS/failexec.sh" <<'A'
#!/usr/bin/env bash
cat >/dev/null
exit 9
A
chmod +x "$ADAPTERS"/*.sh

export PATH="$STUBS:$PATH"
export ORCH_ADAPTER_DIR="$ADAPTERS"

mkissue() { # mkissue <n> <title> [label]
  mkdir -p "$GH_STATE/issues/$1/comments"
  echo "$2" > "$GH_STATE/issues/$1/title"
  echo "Body of issue $1." > "$GH_STATE/issues/$1/body"
  echo "OPEN" > "$GH_STATE/issues/$1/state"
  : > "$GH_STATE/issues/$1/labels"
  [ -n "${3:-}" ] && echo "$3" >> "$GH_STATE/issues/$1/labels"
}

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL: $1"; }
check() { # check <desc> <cmd...>
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}

echo "== A. claim + dispatch, then double-dispatch guard =="
mkissue 42 "Implement widget" "sprint-x"
bash "$HERE/dispatch.sh" 42 stubexec
check "adapter called once" test "$(wc -l < "$GH_STATE/adapter/calls")" = 1
check "claimed label applied" grep -qx claimed "$GH_STATE/issues/42/labels"
check "claim comment posted" grep -ql '^claim: executor=stubexec' "$GH_STATE/issues/42/comments/"*
check "prompt says INIT" grep -q 'MODE: INIT' "$GH_STATE/adapter/last_prompt_42"
check "prompt carries Closes" grep -q "Closes #42" "$GH_STATE/adapter/last_prompt_42"
check "prompt demands progress artifact" grep -q 'progress.log.md' "$GH_STATE/adapter/last_prompt_42"
check "prompt demands baseline check" grep -q 'baseline' "$GH_STATE/adapter/last_prompt_42"
rc=0; bash "$HERE/dispatch.sh" 42 stubexec 2>/dev/null || rc=$?
check "second dispatch exits 3" test "$rc" = 3
check "adapter NOT called again" test "$(wc -l < "$GH_STATE/adapter/calls")" = 1

echo "== B. stale lease sweeps clean and issue is reclaimable =="
# force the lease into the past
for f in "$GH_STATE/issues/42/comments/"*; do
  sed -i 's/expires=[0-9T:Z-]*/expires=2020-01-01T00:00:00Z/' "$f"
done
bash "$HERE/claims.sh" sweep >/dev/null
check "label removed by sweep" bash -c '! grep -qx claimed "$GH_STATE/issues/42/labels"'
check "claim-expired recorded" grep -ql '^claim-expired:' "$GH_STATE/issues/42/comments/"*
bash "$HERE/dispatch.sh" 42 stubexec >/dev/null
check "reclaim after sweep dispatches" test "$(wc -l < "$GH_STATE/adapter/calls")" = 2

echo "== C. resume mode after progress-branch comment =="
mkissue 43 "Resumable task" "sprint-x"
mkdir -p "$GH_STATE/issues/43/comments"
printf 'progress-branch: agent-3/feat/adr-999-widget' > "$GH_STATE/issues/43/comments/1"
bash "$HERE/dispatch.sh" 43 stubexec >/dev/null
check "prompt says RESUME" grep -q 'MODE: RESUME' "$GH_STATE/adapter/last_prompt_43"
check "prompt names the branch" grep -q 'agent-3/feat/adr-999-widget' "$GH_STATE/adapter/last_prompt_43"
check "resume forbids duplicate PR" grep -q 'duplicate PR' "$GH_STATE/adapter/last_prompt_43"

echo "== D. ready.sh excludes claimed, in-flight, and operator-gated =="
mkissue 50 "Free issue" "lbl"
mkissue 51 "Claimed issue" "lbl"
echo "claimed" >> "$GH_STATE/issues/51/labels"
mkissue 52 "In-flight issue" "lbl"
echo "52" > "$GH_STATE/prs_closing"
mkissue 53 "Operator-gated issue" "lbl"
echo "needs-human-input" >> "$GH_STATE/issues/53/labels"
READY=$(bash "$HERE/ready.sh" lbl)
check "lists the free issue" bash -c 'echo "$0" | grep -q "\"number\":50"' "$READY"
check "excludes the claimed issue" bash -c '! echo "$0" | grep -q "\"number\":51"' "$READY"
check "excludes the in-flight issue" bash -c '! echo "$0" | grep -q "\"number\":52"' "$READY"
check "excludes the operator-gated issue" bash -c '! echo "$0" | grep -q "\"number\":53"' "$READY"

echo "== E. adapter failure releases the claim =="
mkissue 60 "Doomed dispatch" "sprint-x"
rc=0; bash "$HERE/dispatch.sh" 60 failexec 2>/dev/null || rc=$?
check "dispatch reports failure" test "$rc" = 1
check "claim released on failure" grep -ql '^claim-released:' "$GH_STATE/issues/60/comments/"*
check "label removed on failure" bash -c '! grep -qx claimed "$GH_STATE/issues/60/labels"'
rc=0; bash "$HERE/dispatch.sh" 60 stubexec >/dev/null 2>&1 || rc=$?
check "issue dispatchable after release" test "$rc" = 0

echo
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
