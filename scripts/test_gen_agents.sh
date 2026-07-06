#!/usr/bin/env bash
# Offline tests for scripts/gen_agents.py (#682): view generation from a
# fixture registry, drift-check semantics, file-vs-directory ownership.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "  ✓ $1"; }
bad()  { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }
check(){ if eval "$2"; then ok "$1"; else bad "$1"; fi; }

ROOT="$TMP/repo"
mkdir -p "$ROOT/docs/agents/registry" "$ROOT/appdir" "$ROOT/.multiclaude/agents"

cat > "$ROOT/docs/agents/registry/x-1.md" <<'EOF'
---
type: Test Agent
title: "X-1: App Owner"
description: Owns the app directory and one tool file.
id: x-1
label: x-1
language: Go
ports: [1234]
owned_paths:
  - appdir/
  - tool.go
depends_on: [x-2]
---

# Charter

Body text with a bundle link to [x-2](/x-2.md).
EOF

cat > "$ROOT/docs/agents/registry/x-2.md" <<'EOF'
---
type: Test Agent
title: "X-2: Advisor"
description: Owns nothing; advises.
id: x-2
label: x-2
---

# Charter

Advisory only.
EOF

GEN="python3 $HERE/gen_agents.py --root $ROOT"

echo "=== generation ==="
OUT=$($GEN 2>&1); RC=$?
check "generator exits 0" "[ $RC -eq 0 ]"
check "module AGENTS.md written for directory-owned path" "[ -f '$ROOT/appdir/AGENTS.md' ]"
check "no per-file view for file-grained ownership" "[ ! -f '$ROOT/tool.go/AGENTS.md' ] && [ ! -f '$ROOT/tool.goAGENTS.md' ]"
check "root AGENTS.md written" "[ -f '$ROOT/AGENTS.md' ]"
check "multiclaude views written (fresh names)" "[ -f '$ROOT/.multiclaude/agents/x-1.md' ] && [ -f '$ROOT/.multiclaude/agents/x-2.md' ]"
check "views carry the GENERATED banner" "grep -q 'GENERATED from docs/agents/registry/x-1.md' '$ROOT/appdir/AGENTS.md'"
check "charter body is inlined" "grep -q 'Body text with a bundle link' '$ROOT/appdir/AGENTS.md'"
check "bundle-absolute links rewritten to full URLs" "grep -q 'blob/main/docs/agents/registry/x-2.md' '$ROOT/appdir/AGENTS.md'"
check "root table carries the file-grained path" "grep -q '\`tool.go\` | x-1' '$ROOT/AGENTS.md'"
check "root anchor points at CLAUDE.md first" "grep -q 'Read it first' '$ROOT/AGENTS.md'"

echo "=== drift check ==="
$GEN --check >"$TMP/c1.txt" 2>&1
check "check is clean immediately after generation" "[ $? -eq 0 ] && grep -q 'clean' '$TMP/c1.txt'"

echo "hand edit" >> "$ROOT/appdir/AGENTS.md"
$GEN --check >"$TMP/c2.txt" 2>&1
check "hand-edited view is flagged as stale (exit 1)" "[ $? -eq 1 ] && grep -q 'appdir/AGENTS.md' '$TMP/c2.txt'"

$GEN >/dev/null 2>&1
$GEN --check >/dev/null 2>&1
check "regeneration heals the drift" "[ $? -eq 0 ]"

echo "=== existing multiclaude filename is reused ==="
ROOT2="$TMP/repo2"
mkdir -p "$ROOT2/docs/agents/registry" "$ROOT2/.multiclaude/agents" "$ROOT2/appdir"
cp "$ROOT/docs/agents/registry/x-1.md" "$ROOT2/docs/agents/registry/"
touch "$ROOT2/.multiclaude/agents/x-1-legacyname.md"
python3 "$HERE/gen_agents.py" --root "$ROOT2" >/dev/null 2>&1
check "pre-existing <id>-slug filename is regenerated in place" "grep -q 'GENERATED' '$ROOT2/.multiclaude/agents/x-1-legacyname.md' && [ ! -f '$ROOT2/.multiclaude/agents/x-1.md' ]"

echo "=== required-field validation ==="
ROOT3="$TMP/repo3"
mkdir -p "$ROOT3/docs/agents/registry"
cat > "$ROOT3/docs/agents/registry/x-3.md" <<'EOF'
---
type: Test Agent
title: "X-3: No Label"
id: x-3
---

# Charter
EOF
if python3 "$HERE/gen_agents.py" --root "$ROOT3" >"$TMP/v.txt" 2>&1; then RCV=0; else RCV=$?; fi
check "missing label is a clear diagnostic, not a traceback (exit 2)" "[ $RCV -eq 2 ] && grep -q 'missing required field(s): label' '$TMP/v.txt' && ! grep -q 'Traceback' '$TMP/v.txt'"

echo ""
echo "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
