#!/usr/bin/env bash
# Offline tests for scripts/generate_governance_onboarding.py — no network,
# no gh. Style mirrors scripts/orchestration/test_dispatch.sh.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
GEN="$ROOT/scripts/generate_governance_onboarding.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); echo "  ✓ $1"; }
bad()  { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }
check(){ if eval "$2"; then ok "$1"; else bad "$1"; fi; }

echo "=== generate: default out dir ==="
OUT="$TMP/out"
python3 "$GEN" --out "$OUT" >"$TMP/gen.log" 2>&1
check "generator exits 0" "[ $? -eq 0 ]"

check "host repo (kaizen-experimentation) NOT generated" \
  "[ ! -d '$OUT/kaizen-experimentation' ]"
check "kaizen-recsys generated" "[ -d '$OUT/kaizen-recsys' ]"

SIBLINGS=$(find "$OUT" -mindepth 1 -maxdepth 1 -type d | wc -l)
check "7 sibling repos generated (8 fleet - host)" "[ '$SIBLINGS' -eq 7 ]"

for f in workflows/review-gate.yml workflows/pr-title.yml workflows/automerge.yml rulesets/main.json; do
  check "kaizen-recsys has .github/$f" "[ -f '$OUT/kaizen-recsys/.github/$f' ]"
done
check "kaizen-recsys has README.md" "[ -f '$OUT/kaizen-recsys/README.md' ]"

echo "=== generated content ==="
check "callers reference owner-qualified reusable @main" \
  "grep -q 'uses: wunderkennd/kaizen-experimentation/.github/workflows/_review-gate.yml@main' '$OUT/kaizen-recsys/.github/workflows/review-gate.yml'"
check "no pull_request_review_thread trigger (validator rejects it; comments may mention it)" \
  "! grep -rqE '^[[:space:]]*pull_request_review_thread:' '$OUT'"
check "all generated workflows parse as YAML" \
  "python3 -c \"import yaml,glob; [yaml.safe_load(open(f)) for f in glob.glob('$OUT/*/.github/workflows/*.yml')]\""

python3 - "$OUT" <<'EOF'
import json, sys, pathlib
out = pathlib.Path(sys.argv[1])
rs = json.loads((out / "kaizen-recsys/.github/rulesets/main.json").read_text())
checks = [r for r in rs["rules"] if r["type"] == "required_status_checks"][0]
contexts = [c["context"] for c in checks["parameters"]["required_status_checks"]]
assert contexts == ["PR title check / check", "Review gate / gate"], contexts
assert rs["enforcement"] == "disabled", rs["enforcement"]
EOF
check "sibling ruleset: exactly the 2 governance contexts, enforcement disabled" "[ $? -eq 0 ]"

echo "=== apply mode ==="
PARENT="$TMP/parent"
mkdir -p "$PARENT/kaizen-recsys"
python3 "$GEN" --out "$TMP/out2" --apply "$PARENT" >"$TMP/apply1.log" 2>&1
check "apply exits 0" "[ $? -eq 0 ]"
check "apply copied caller into existing checkout" \
  "[ -f '$PARENT/kaizen-recsys/.github/workflows/review-gate.yml' ]"
check "apply did NOT copy README into checkout" \
  "[ ! -f '$PARENT/kaizen-recsys/README.md' ]"
check "apply skipped repos without checkouts" \
  "grep -q 'skip kaizen-pipelines: no checkout' '$TMP/apply1.log'"

python3 "$GEN" --out "$TMP/out3" --apply "$PARENT" >"$TMP/apply2.log" 2>&1
check "second apply reports unchanged (idempotent)" \
  "grep -q 'unchanged .*kaizen-recsys/.github/workflows/review-gate.yml' '$TMP/apply2.log'"

echo "=== workflows ref override ==="
GOVERNANCE_WORKFLOWS_REF="wunderkind-ventures/kaizen-experimentation@main" \
  python3 "$GEN" --out "$TMP/out4" >/dev/null 2>&1
check "GOVERNANCE_WORKFLOWS_REF re-points callers (org-transfer path)" \
  "grep -q 'uses: wunderkind-ventures/kaizen-experimentation/.github/workflows/_pr-title.yml@main' '$TMP/out4/kaizen-recsys/.github/workflows/pr-title.yml'"

echo ""
echo "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
