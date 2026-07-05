#!/usr/bin/env bash
# Offline tests for the H2 native work-graph (#692): ready.sh's native path,
# fallback behavior, READY_DRIFT mode, and the blocked-by migration script.
# Style mirrors test_dispatch.sh — a filesystem-backed gh stub, no network.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$(dirname "$HERE")")"
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
    if [ "${STUB_GRAPHQL_FAIL:-0}" = "1" ]; then
      echo "GraphQL error" >&2; exit 1
    fi
    cat "$STUB/graphql.json" ;;
  "issue list --label "*"--state open --limit 200 --json number,title,body")
    cat "$STUB/legacy_list.json" ;;
  "issue list --state open --limit 500 --json number,body --jq"*)
    cat "$STUB/mig_open_lines.txt" ;;
  "issue view "*"--json state -q .state")
    n=$(echo "$*" | grep -oE 'view [0-9]+' | grep -oE '[0-9]+')
    cat "$STUB/state_$n" 2>/dev/null || echo "MISSING" ;;
  "pr list"*)
    echo "[]" ;;
  "issue list --label claimed"*)
    echo "[]" ;;
  "api --paginate repos/stub/repo/issues?state=all&per_page=100 --jq"*)
    cat "$STUB/mig_index.tsv" ;;
  "api repos/stub/repo/issues/"*"/dependencies/blocked_by -X POST -F issue_id="*)
    tgt=$(echo "$*" | grep -oE 'issues/[0-9]+' | grep -oE '[0-9]+')
    if [ -f "$STUB/edge_exists_$tgt" ]; then
      echo '{"message":"Validation Failed","errors":["Dependency already exists"]}' >&2; exit 1
    fi
    echo "EDGE $*" >> "$STUB/edges.log"
    echo '{}' ;;
  *)
    echo "[]" ;;
esac
GHEOF
chmod +x "$STUB/bin/gh"
export PATH="$STUB/bin:$PATH"

# --- fixtures: native cohort -------------------------------------------------
# #1 plain-ready · #2 claimed · #3 blocked by OPEN #9 · #4 in-flight PR ·
# #5 blocker CLOSED → ready
cat > "$STUB/graphql.json" <<'EOF'
{"data":{"repository":{"issues":{"nodes":[
 {"number":1,"title":"plain ready","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[]},"closedByPullRequestsReferences":{"nodes":[]}},
 {"number":2,"title":"claimed","labels":{"nodes":[{"name":"sprint-9"},{"name":"claimed"}]},"blockedBy":{"nodes":[]},"closedByPullRequestsReferences":{"nodes":[]}},
 {"number":3,"title":"blocked open","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[{"number":9,"state":"OPEN"}]},"closedByPullRequestsReferences":{"nodes":[]}},
 {"number":4,"title":"in flight","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[]},"closedByPullRequestsReferences":{"nodes":[{"number":77}]}},
 {"number":5,"title":"blocker closed","labels":{"nodes":[{"name":"sprint-9"}]},"blockedBy":{"nodes":[{"number":8,"state":"CLOSED"}]},"closedByPullRequestsReferences":{"nodes":[]}}
]}}}}
EOF

echo "=== native path ==="
OUT=$(bash "$HERE/ready.sh" sprint-9 2>/dev/null)
NUMS=$(printf '%s\n' "$OUT" | jq -r '.number' | tr '\n' ' ')
check "native path returns exactly the ready set (1, 5)" "[ '$NUMS' = '1 5 ' ]"
check "claimed excluded via labels-in-query" "! grep -q '\"number\":2' <<<'$OUT'"
check "open-blocker excluded via blockedBy" "! grep -q '\"number\":3' <<<'$OUT'"
check "in-flight excluded via closedByPullRequestsReferences" "! grep -q '\"number\":4' <<<'$OUT'"
check "native path made exactly one graphql call" "[ \$(grep -c 'api graphql' '$CALLLOG') -eq 1 ]"

echo "=== fallback on native failure ==="
cat > "$STUB/legacy_list.json" <<'EOF'
[{"number":42,"title":"legacy ready","body":"## Blocked by\n\n- #40\n"}]
EOF
echo "CLOSED" > "$STUB/state_40"
: > "$CALLLOG"
OUT2=$(STUB_GRAPHQL_FAIL=1 bash "$HERE/ready.sh" sprint-9 2>"$TMP/warn.txt")
check "graphql failure falls back to body-parse" "grep -q '\"number\":42' <<<'$OUT2'"
check "fallback emits the PA-residual warning" "grep -q 'PA-residual' '$TMP/warn.txt'"

echo "=== drift mode ==="
# Aligned: legacy list mirrors native's ready set {1,5}
cat > "$STUB/legacy_list.json" <<'EOF'
[{"number":1,"title":"plain ready","body":""},
 {"number":5,"title":"blocker closed","body":"## Blocked by\n\n- #8\n"}]
EOF
echo "CLOSED" > "$STUB/state_8"
READY_DRIFT=1 bash "$HERE/ready.sh" sprint-9 >"$TMP/drift1.txt" 2>&1
check "drift clean exits 0 and says clean" "[ $? -eq 0 ] && grep -q 'clean' '$TMP/drift1.txt'"

cat > "$STUB/legacy_list.json" <<'EOF'
[{"number":1,"title":"plain ready","body":""}]
EOF
READY_DRIFT=1 bash "$HERE/ready.sh" sprint-9 >"$TMP/drift2.txt" 2>&1
check "drift mismatch exits 1 with MISMATCH" "[ $? -eq 1 ] && grep -q 'MISMATCH' '$TMP/drift2.txt'"

STUB_GRAPHQL_FAIL=1 READY_DRIFT=1 bash "$HERE/ready.sh" sprint-9 >"$TMP/drift3.txt" 2>&1
check "drift with native unavailable exits 2 (evidence gap)" "[ $? -eq 2 ]"

echo "=== migration script ==="
MIG="$ROOT/scripts/projects/migrate-blocked-by-to-dependencies.sh"
# Index: #10 open, #11 open, #12 closed. Open issue #10 blocked by #11 (open),
# #12 (closed), other/repo#3 (cross-repo), missing #99.
printf '10\topen\t1010\n11\topen\t1011\n12\tclosed\t1012\n' > "$STUB/mig_index.tsv"
cat > "$STUB/mig_open_lines.txt" <<'EOF'
{"number":10,"body":"## Blocked by\n\n- #11\n- #12\n- other/repo#3\n- #99\n"}
EOF
: > "$STUB/edges.log"
bash "$MIG" > "$TMP/mig_dry.txt" 2>&1
check "dry-run exits 0" "[ $? -eq 0 ]"
check "dry-run plans the open-blocker edge only" "grep -q '#10 blocked_by #11: WOULD create' '$TMP/mig_dry.txt'"
check "dry-run skips closed blocker" "grep -q '#10 blocked_by #12: blocker closed' '$TMP/mig_dry.txt'"
check "dry-run flags cross-repo ref" "grep -q 'UNSUPPORTED' '$TMP/mig_dry.txt'"
check "dry-run writes nothing" "[ ! -s '$STUB/edges.log' ]"

bash "$MIG" --apply > "$TMP/mig_apply.txt" 2>&1
check "apply exits 0" "[ $? -eq 0 ]"
check "apply created the edge" "grep -q '#10 blocked_by #11: created' '$TMP/mig_apply.txt' && grep -q 'EDGE' '$STUB/edges.log'"

touch "$STUB/edge_exists_10"
bash "$MIG" --apply > "$TMP/mig_apply2.txt" 2>&1
check "re-apply treats existing edge as skip (idempotent)" "[ $? -eq 0 ] && grep -q 'already exists — skip' '$TMP/mig_apply2.txt'"

echo ""
echo "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
