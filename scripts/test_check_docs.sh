#!/usr/bin/env bash
# Offline tests for scripts/check_docs.py — fixture corpus in a temp root.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LINT="$HERE/check_docs.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "  ✓ $1"; }
bad()  { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }
check(){ if eval "$2"; then ok "$1"; else bad "$1"; fi; }

R="$TMP/repo"
mkdir -p "$R/docs/adrs" "$R/docs/superpowers/plans" "$R/docs/superpowers/specs" "$R/docs/prds" "$R/docs/templates"

# Clean fixtures
printf '# ADR-001: Clean\n\n**Status**: Accepted\n**Date**: 2026-01-01\n\n## Context\n' > "$R/docs/adrs/001-clean.md"
cat > "$R/docs/superpowers/plans/2026-07-06-post-v2-clean.md" <<'EOF'
# Post-v2 plan (#1)
**Plan-review:** linked
## Platform assumptions & probes
| PA1 | none new |
## Locks — binding for implementers
## Phase A — only phase
EOF
printf -- '---\ntype: PRD\n---\n# PRD\n- **Primary metric**: X > 5\n' > "$R/docs/prds/2026-07-06-clean.md"

python3 "$LINT" "$R" >"$TMP/out1" 2>&1
check "clean corpus exits 0" "[ $? -eq 0 ]"
check "clean corpus: 0 errors 0 warnings" "grep -q '0 error(s), 0 warning(s)' '$TMP/out1'"

# Post-v2 multi-phase plan missing v2 sections -> ERRORS
cat > "$R/docs/superpowers/plans/2026-07-07-post-v2-bad.md" <<'EOF'
# Bad plan
## Phase A — one
## Phase B — two
EOF
python3 "$LINT" "$R" >"$TMP/out2" 2>&1
check "post-v2 violations exit 1" "[ $? -eq 1 ]"
check "missing Cross-phase table is an ERROR post-v2" "grep -q 'ERROR.*Cross-phase artifacts' '$TMP/out2'"
check "missing probes section is an ERROR post-v2" "grep -q 'ERROR.*Platform assumptions' '$TMP/out2'"
rm "$R/docs/superpowers/plans/2026-07-07-post-v2-bad.md"

# Pre-v2 plan with same gaps -> warnings only (grandfathered)
cat > "$R/docs/superpowers/plans/2026-05-01-legacy.md" <<'EOF'
# Legacy plan
## Phase A — one
## Phase B — two
EOF
python3 "$LINT" "$R" >"$TMP/out3" 2>&1
check "grandfathered corpus exits 0 by default" "[ $? -eq 0 ]"
check "legacy gaps are warnings" "grep -q 'warning.*grandfathered' '$TMP/out3'"
DOCS_LINT_STRICT=1 python3 "$LINT" "$R" >/dev/null 2>&1
check "strict mode escalates warnings to exit 1" "[ $? -eq 1 ]"
rm "$R/docs/superpowers/plans/2026-05-01-legacy.md"

# PRD with two primary metrics -> error
printf -- '---\ntype: PRD\n---\n# PRD\n- **Primary metric**: A\n- **Primary metric**: B\n' > "$R/docs/prds/2026-07-06-twometrics.md"
python3 "$LINT" "$R" >"$TMP/out4" 2>&1
check "two primary metrics is an ERROR (Goal rule)" "grep -q 'ERROR.*exactly ONE' '$TMP/out4'"
rm "$R/docs/prds/2026-07-06-twometrics.md"

# ADR without markers -> warning
printf '# ADR-002: No markers\n\ntext\n' > "$R/docs/adrs/002-nomarkers.md"
python3 "$LINT" "$R" >"$TMP/out5" 2>&1
check "ADR without Status/Date warns, still exit 0" "[ $? -eq 0 ] && grep -q 'warning.*Status' '$TMP/out5'"

# Live corpus: no ERRORS (warnings allowed)
python3 "$LINT" "$HERE/.." >"$TMP/out6" 2>&1
check "live corpus has zero ERRORS" "grep -q ' 0 error(s)' '$TMP/out6'"

echo ""
echo "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
