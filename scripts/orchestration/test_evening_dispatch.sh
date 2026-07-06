#!/usr/bin/env bash
# Offline tests for the H4 evening dispatcher (#716): shadow reporting,
# dedupe + cap + ordering, the live path via a stubbed dispatch entrypoint,
# and the claude-workflow adapter. gh-stub pattern per test_ready_native.sh.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "  ✓ $1"; }
bad()  { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }
check(){ if eval "$2"; then ok "$1"; else bad "$1"; fi; }

# --- gh stub -----------------------------------------------------------------
export STUB="$TMP/stub"
mkdir -p "$STUB/bin"
export CALLLOG="$STUB/calls.log"
: > "$CALLLOG"

cat > "$STUB/bin/gh" <<'GHEOF'
#!/usr/bin/env bash
echo "$*" >> "$CALLLOG"
case "$*" in
  "repo view --json nameWithOwner -q .nameWithOwner")
    echo "stub/repo" ;;
  "api graphql"*)
    cat "$STUB/graphql.json" ;;
  "issue list --state open --limit 200 --json labels --jq"*)
    cat "$STUB/labels.txt" 2>/dev/null || true ;;
  "workflow run claude-worker.yml -f issue="*)
    echo "WFRUN $*" >> "$STUB/wfrun.log" ;;
  *)
    echo "[]" ;;
esac
GHEOF
chmod +x "$STUB/bin/gh"
export PATH="$STUB/bin:$PATH"

# Native _ready fixture: #1 and #5 ready; #2 claimed; #3 blocked; #4 in-flight.
cat > "$STUB/graphql.json" <<'EOF'
{"data":{"repository":{"issues":{"nodes":[
 {"number":5,"title":"ready five","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[]},"closedByPullRequestsReferences":{"nodes":[]}},
 {"number":1,"title":"ready one","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[]},"closedByPullRequestsReferences":{"nodes":[]}},
 {"number":2,"title":"claimed","labels":{"nodes":[{"name":"sprint-9"},{"name":"claimed"}]},"blockedBy":{"nodes":[]},"closedByPullRequestsReferences":{"nodes":[]}},
 {"number":3,"title":"blocked","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[{"number":9,"state":"OPEN"}]},"closedByPullRequestsReferences":{"nodes":[]}}
]}}}}
EOF

ED="$HERE/evening_dispatch.sh"

echo "=== shadow mode ==="
OUT=$(bash "$ED" sprint-9 2>&1)
check "shadow reports both ready issues" "grep -q 'would dispatch #1' <<<'$OUT' && grep -q 'would dispatch #5' <<<'$OUT'"
check "shadow excludes claimed/blocked" "! grep -q '#2' <<<'$OUT' && ! grep -q '#3' <<<'$OUT'"
check "shadow launches nothing" "[ ! -f '$STUB/wfrun.log' ]"
check "shadow posts no comments" "! grep -q 'issue comment' '$CALLLOG'"
check "shadow orders ascending" "[ \"\$(grep -o 'would dispatch #[0-9]*' <<<'$OUT' | head -1)\" = 'would dispatch #1' ]"

echo "=== dedupe across cohorts ==="
OUT2=$(bash "$ED" sprint-9 sprint-8 2>&1)
check "issue appears once despite two cohorts" "[ \$(grep -c 'would dispatch #1' <<<'$OUT2') -eq 1 ]"

echo "=== cap ==="
OUT3=$(DISPATCH_CAP=1 bash "$ED" sprint-9 2>&1)
check "cap=1 selects exactly one" "[ \$(grep -c 'would dispatch #' <<<'$OUT3') -eq 1 ]"
check "cap overflow is reported" "grep -q '1 more ready beyond the cap' <<<'$OUT3'"

echo "=== cohort discovery ==="
printf 'sprint-9\n' > "$STUB/labels.txt"
OUT4=$(bash "$ED" 2>&1)
check "no-arg run discovers sprint-* cohorts" "grep -q 'cohorts: sprint-9' <<<'$OUT4' && grep -q 'would dispatch #1' <<<'$OUT4'"

: > "$STUB/labels.txt"
OUT5=$(bash "$ED" 2>&1); RC5=$?
check "no cohorts → clean no-op" "[ $RC5 -eq 0 ] && grep -q 'no open sprint-\*' <<<'$OUT5'"

echo "=== live mode (stubbed dispatch.sh) ==="
cat > "$STUB/bin/dispatch_stub" <<'DSEOF'
#!/usr/bin/env bash
echo "DISPATCH $*" >> "$DISPATCHLOG"
[ -f "$STUB/claimed_$1" ] && exit 3
[ -f "$STUB/fail_$1" ] && exit 1
exit 0
DSEOF
chmod +x "$STUB/bin/dispatch_stub"
export DISPATCHLOG="$STUB/dispatch.log"

: > "$DISPATCHLOG"
OUT6=$(DISPATCH_BIN="$STUB/bin/dispatch_stub" bash "$ED" --live sprint-9 2>&1); RC6=$?
check "live dispatches each selected issue via claude-workflow" "[ \$(grep -c 'claude-workflow' '$DISPATCHLOG') -eq 2 ] && [ $RC6 -eq 0 ]"
check "live summary reports dispatched=2" "grep -q 'dispatched=2 already-claimed=0 failed=0' <<<'$OUT6'"

touch "$STUB/claimed_1"
: > "$DISPATCHLOG"
OUT7=$(DISPATCH_BIN="$STUB/bin/dispatch_stub" bash "$ED" --live sprint-9 2>&1); RC7=$?
check "claim collision (rc=3) is a skip, not a failure" "[ $RC7 -eq 0 ] && grep -q 'already claimed' <<<'$OUT7'"

touch "$STUB/fail_5"
OUT8=$(DISPATCH_BIN="$STUB/bin/dispatch_stub" bash "$ED" --live sprint-9 2>&1); RC8=$?
check "adapter failure makes the run fail" "[ $RC8 -ne 0 ] && grep -q 'FAILED to dispatch' <<<'$OUT8'"
rm -f "$STUB/claimed_1" "$STUB/fail_5"

echo "=== claude-workflow adapter ==="
ADP="$HERE/dispatch.d/claude-workflow.sh"
printf 'do the task' | bash "$ADP" 42
check "adapter launches the worker workflow with issue + prompt" "grep -q 'workflow run claude-worker.yml -f issue=42 -f prompt=do the task' '$STUB/wfrun.log'"

BIG=$(printf 'x%.0s' $(seq 1 60001))
if printf '%s' "$BIG" | bash "$ADP" 42 2>"$TMP/adp_err.txt"; then RC9=0; else RC9=$?; fi
check "adapter refuses an oversize prompt (L8)" "[ $RC9 -ne 0 ] && grep -q '60000 budget' '$TMP/adp_err.txt'"

echo ""
echo "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
