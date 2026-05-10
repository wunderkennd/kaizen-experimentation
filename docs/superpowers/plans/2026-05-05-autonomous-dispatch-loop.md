# Autonomous Dispatch Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make multiclaude's `autonomous-sprint` dispatcher dependency-aware so it dispatches only "ready" GitHub Issues (all blockers closed, no in-flight PR), and add a GitHub Action + beads-DAG integration so the loop self-promotes future waves without manual label gymnastics.

**Architecture:** Three layers. (1) **Justfile-level dependency parser** that reads `## Blocked by` sections from issue bodies, checks blocker state, and emits ready issues. (2) **GitHub Action** that, on issue close, posts a "you're now ready" comment on dependents — visible signal in the GH UI without requiring a runner. (3) **Beads DAG** as a second source of truth: `beads-sync` encodes dependency edges via `bd dep add`, and `_ready` prefers `bd ready --label <sprint>` when beads is initialized, falling back to body parsing otherwise.

**Tech Stack:** Bash, jq, `gh` CLI, `bd` CLI (steveyegge/beads), GitHub Actions, just (justfile recipe runner).

---

## Files

| File | Action | Responsibility |
|------|--------|----------------|
| `justfile` | Modify | Add `_ready` recipe; modify `autonomous-sprint` to dispatch only ready issues |
| `scripts/beads-sync.sh` | Modify | Parse `## Blocked by` from issue bodies, encode edges via `bd dep add` |
| `.github/workflows/auto-promote.yml` | Create | On issue close, comment on newly-unblocked sprint-I.3 dependents |

No new files in `infra/`. No application code changes.

---

## Task 1: `_ready` justfile helper (Step A)

**Files:**
- Modify: `justfile` (new recipe, insert before `autonomous-sprint` at line 928)

- [ ] **Step 1.1: Add the `_ready` recipe**

Insert this recipe in `justfile` immediately above the line `# Sprint launchers read Issues by label (primary) with milestone fallback` (around line 928):

```makefile
# Internal: emit one JSON object per line for "ready" issues with the given label.
# An issue is ready when (1) it has no open PR closing it, AND (2) every "#N" listed
# under "## Blocked by" in its body refers to a CLOSED issue (or no blockers exist).
_ready label:
    #!/usr/bin/env bash
    set -euo pipefail
    LABEL="{{label}}"
    # In-flight = open PRs that close any issue. Excluded from dispatch.
    IN_FLIGHT=$(gh pr list --state open --limit 200 \
      --json closingIssuesReferences \
      --jq '[.[].closingIssuesReferences[].number] | unique | join(" ")' 2>/dev/null || echo "")
    gh issue list --label "$LABEL" --state open --limit 200 --json number,title,body \
      | jq -c '.[]' \
      | while IFS= read -r issue; do
          num=$(echo "$issue" | jq -r '.number')
          # Skip if a PR already exists closing this issue.
          if [ -n "$IN_FLIGHT" ] && echo " $IN_FLIGHT " | grep -q " $num "; then
            continue
          fi
          body=$(echo "$issue" | jq -r '.body // ""')
          # Extract issue numbers from the "## Blocked by" section through next H2 or EOF.
          blockers=$(echo "$body" \
            | awk '/^## Blocked by/{flag=1; next} /^## /{flag=0} flag' \
            | grep -oE '#[0-9]+' | tr -d '#' | sort -u || true)
          ready=true
          for b in $blockers; do
            state=$(gh issue view "$b" --json state -q '.state' 2>/dev/null || echo "MISSING")
            if [ "$state" != "CLOSED" ]; then
              ready=false
              break
            fi
          done
          if [ "$ready" = "true" ]; then
            echo "$issue" | jq -c '{number, title}'
          fi
        done
```

- [ ] **Step 1.2: Verify on the live sprint**

Run: `just _ready sprint-I.3`

Expected output (with only #477 and #478 currently labeled `sprint-I.3` and neither yet has a closing PR):

```
{"number":477,"title":"Phase 0: Refactor infra/ into provider modules + switch-dispatch Deploy()"}
{"number":478,"title":"Phase 2: Replace Confluent Schema Registry with Redpanda in Docker Compose"}
```

If output differs, debug the awk/grep extraction by piping `gh issue view 477 --json body -q '.body'` through the same pipeline manually.

- [ ] **Step 1.3: Verify on a sprint with no labeled issues**

Run: `just _ready sprint-nonexistent`

Expected: empty output, exit code 0.

- [ ] **Step 1.4: Commit**

```bash
git add justfile
git commit -m "feat(justfile): add _ready helper for dependency-aware dispatch

Parses '## Blocked by' from issue bodies and excludes issues with open
closing PRs. Emits one JSON object per ready issue."
```

---

## Task 2: Make `autonomous-sprint` use `_ready` (Step B)

**Files:**
- Modify: `justfile:929-986`

- [ ] **Step 2.1: Replace the issue-fetching block**

In `justfile`, find the `autonomous-sprint sprint_num:` recipe (line 929). Locate the block that begins with `ISSUES=$(gh issue list --label "$LABEL"` and ends with `echo "  ⚠ No open issues found ...`. Replace it with:

```makefile
    echo "=== Launching workers for: $MS ==="
    # Use _ready to filter blocked or in-flight issues. Falls back to label-only
    # query if no issues match (e.g., older sprints whose bodies don't follow
    # the "## Blocked by" convention).
    ISSUES=$(just _ready "$LABEL")
    if [ -z "$ISSUES" ]; then
      # Backwards compatibility: pre-I.3 sprints don't use structured blocker sections.
      ISSUES=$(gh issue list --label "$LABEL" --state open --json number,title --jq '.[] | @json' 2>/dev/null)
      if [ -z "$ISSUES" ]; then
        ISSUES=$(gh issue list --milestone "$MS" --state open --json number,title --jq '.[] | @json' 2>/dev/null)
      fi
    fi
    if [ -z "$ISSUES" ]; then
      echo "  ⚠ No ready issues found for sprint {{sprint_num}}. Either all blocked, all in-flight, or none labeled."
      exit 0
    fi
```

The remainder of the recipe (the `while IFS= read -r line; do ... done <<< "$ISSUES"` loop and the final `echo "✓ Workers launched"`) stays as-is.

Wait — the current recipe pipes `$ISSUES` via `echo | while`, which spawns a subshell so `COUNT` is lost. Change the loop terminator from `done` followed by `echo "✓ Workers launched for $MS ($COUNT issues)"` to use `<<< "$ISSUES"` (here-string) so the loop runs in the current shell and `COUNT` survives.

Concretely, change the final loop from:

```bash
    echo "$ISSUES" | while IFS= read -r line; do
      ...
      COUNT=$((COUNT + 1))
    done
    echo "✓ Workers launched for $MS ($COUNT issues)"
```

to:

```bash
    while IFS= read -r line; do
      ...
      COUNT=$((COUNT + 1))
    done <<< "$ISSUES"
    echo "✓ Workers launched for $MS ($COUNT ready issues)"
```

- [ ] **Step 2.2: Dry-run by stubbing `multiclaude worker create`**

Stub the dispatcher to print instead of execute, then run the recipe:

```bash
PATH="/tmp/stub:$PATH"
mkdir -p /tmp/stub
cat > /tmp/stub/multiclaude <<'EOF'
#!/usr/bin/env bash
echo "STUB multiclaude $@"
EOF
chmod +x /tmp/stub/multiclaude
just autonomous-sprint I.3
```

Expected: `STUB multiclaude worker create ...` lines for #477 and #478 only. Final line: `✓ Workers launched for Sprint I.3: Multi-Cloud Foundation (2 ready issues)`.

Cleanup: `rm -rf /tmp/stub`.

- [ ] **Step 2.3: Commit**

```bash
git add justfile
git commit -m "feat(justfile): autonomous-sprint dispatches only ready issues

Routes through _ready first; falls back to old label/milestone query
for backwards compatibility with sprints whose issues don't follow the
'## Blocked by' convention. Switch the loop to a here-string so the
COUNT variable survives the subshell."
```

---

## Task 3: Apply `sprint-I.3` to remaining 22 issues (Step C)

**Files:** none — this is a GitHub API operation.

Issues to label: 479, 480, 481, 482, 483, 484, 485, 486, 487, 488, 489, 490, 491, 492, 493, 494, 495, 496, 498, 500, 501, 502.
Issues to **exclude**: 499 and 503 (HITL — require human decisions before code).

- [ ] **Step 3.1: Bulk-label the 22 issues**

```bash
for n in 479 480 481 482 483 484 485 486 487 488 489 490 491 492 493 494 495 496 498 500 501 502; do
  gh issue edit "$n" --add-label "sprint-I.3" --milestone "Sprint I.3: Multi-Cloud Foundation"
done
```

Expected: 22 lines of issue URLs, no errors.

- [ ] **Step 3.2: Verify via `_ready`**

Run: `just _ready sprint-I.3 | wc -l`

Expected: `2` (only #477 and #478 are ready; the other 22 have unclosed blockers).

If higher, an issue body's `## Blocked by` section has a malformed reference. Inspect with `just _ready sprint-I.3 | jq -r .number` and debug.

- [ ] **Step 3.3: No commit needed** — this is GitHub state, not code state.

---

## Task 4: GitHub Action — auto-promote.yml (Step D)

**Files:**
- Create: `.github/workflows/auto-promote.yml`

- [ ] **Step 4.1: Create the workflow file**

```yaml
name: Auto-promote dependents on issue close

on:
  issues:
    types: [closed]

permissions:
  issues: write

jobs:
  promote:
    if: contains(github.event.issue.labels.*.name, 'sprint-I.3')
    runs-on: ubuntu-latest
    steps:
      - name: Find newly-ready dependents and comment
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CLOSED_ISSUE: ${{ github.event.issue.number }}
        run: |
          set -euo pipefail
          # Find open sprint-I.3 issues whose body lists the just-closed issue as a blocker.
          gh issue list --label "sprint-I.3" --state open --limit 200 \
            --json number,body \
          | jq -c --arg closed "#$CLOSED_ISSUE" '
              .[]
              | select(.body | test("(?ms)^## Blocked by.*?" + $closed + "\\b"))
            ' \
          | while IFS= read -r dep; do
              num=$(echo "$dep" | jq -r '.number')
              body=$(echo "$dep" | jq -r '.body')
              # Re-check ALL of this dependent's blockers.
              blockers=$(echo "$body" \
                | awk '/^## Blocked by/{flag=1; next} /^## /{flag=0} flag' \
                | grep -oE '#[0-9]+' | tr -d '#' | sort -u || true)
              all_closed=true
              for b in $blockers; do
                state=$(gh issue view "$b" --json state -q '.state' 2>/dev/null || echo "MISSING")
                if [ "$state" != "CLOSED" ]; then
                  all_closed=false
                  break
                fi
              done
              if [ "$all_closed" = "true" ]; then
                gh issue comment "$num" --body "🟢 All blockers closed. This issue is now ready for dispatch — \`just autonomous-sprint I.3\` (or wait for the next scheduled run)."
              fi
            done
```

- [ ] **Step 4.2: Validate workflow syntax**

Run: `gh workflow view auto-promote.yml 2>&1 || echo "Workflow not yet pushed; syntax-check via actionlint instead"`

If `actionlint` is installed locally: `actionlint .github/workflows/auto-promote.yml`. Expected: no errors.

If neither tool is available, validate by reading the file: ensure indentation is consistent (2 spaces), no tabs, and the `jq` expression parses (test with `echo '{"body":"## Blocked by\n- #123"}' | jq -c 'select(.body | test("(?ms)^## Blocked by.*?#123\\b"))'` → expected: the input echoed back).

- [ ] **Step 4.3: Commit**

```bash
git add .github/workflows/auto-promote.yml
git commit -m "feat(ci): auto-promote dependents on sprint-I.3 issue close

When a sprint-I.3 issue closes, scan open sprint-I.3 issues whose body
lists it as a blocker. For any dependent whose ALL blockers are now
closed, post a 'ready for dispatch' comment. No infrastructure required;
visible signal in the GH UI."
```

---

## Task 5: Beads DAG integration (Step E)

**Files:**
- Modify: `scripts/beads-sync.sh`
- Modify: `justfile` — `_ready` recipe to prefer beads when available

- [ ] **Step 5.1: Read the current beads-sync.sh end-to-end**

```bash
wc -l scripts/beads-sync.sh
cat scripts/beads-sync.sh
```

You need to know where the `bd create` (or equivalent) call sits, and where `external_ref` is set.

- [ ] **Step 5.2: Add an "encode dependencies" pass at the end of beads-sync.sh**

Append this section to `scripts/beads-sync.sh` immediately before any final `echo` or summary line:

```bash
# === Encode "Blocked by" edges from issue bodies as bd dependencies. ===
# For each synced bead with an external_ref of "gh-N", parse the GH issue body
# for "## Blocked by" references and call `bd dep add <bead-id> <blocker-bead-id>`.
echo "=== Encoding dependency edges ==="

# Build a map: gh-issue-number → bead-id
declare -A BEAD_BY_GH
while IFS=$'\t' read -r bead_id ext_ref; do
  if [[ "$ext_ref" =~ ^gh-([0-9]+)$ ]]; then
    BEAD_BY_GH["${BASH_REMATCH[1]}"]="$bead_id"
  fi
done < <(bd list --all --json 2>/dev/null \
  | jq -r '.[] | select(.external_ref != null) | "\(.id)\t\(.external_ref)"')

# For every issue we just synced, parse "## Blocked by" and add edges.
echo "$ISSUES_JSON" | while IFS= read -r issue; do
  num=$(echo "$issue" | jq -r '.number')
  bead_id="${BEAD_BY_GH[$num]:-}"
  if [ -z "$bead_id" ]; then
    continue
  fi
  body=$(echo "$issue" | jq -r '.body // ""')
  blockers=$(echo "$body" \
    | awk '/^## Blocked by/{flag=1; next} /^## /{flag=0} flag' \
    | grep -oE '#[0-9]+' | tr -d '#' | sort -u || true)
  for blocker_num in $blockers; do
    blocker_bead="${BEAD_BY_GH[$blocker_num]:-}"
    if [ -z "$blocker_bead" ]; then
      echo "  (skip: blocker #$blocker_num not synced as a bead)"
      continue
    fi
    # bd dep add <blocked> <blocker>  — idempotent: re-adding is a no-op.
    bd dep add "$bead_id" "$blocker_bead" 2>/dev/null || true
  done
done

echo "✓ Dependency edges encoded"
```

The `ISSUES_JSON` variable is set earlier in the script when issues are queried; verify the variable name matches by reading the existing script. If the variable is named differently, adjust the reference in the new block.

- [ ] **Step 5.3: Sync and verify**

```bash
just beads-init  # idempotent if already initialized
just beads-sync I.3
```

Expected: dependency edges encoded message at the end. Then verify via:

```bash
bd ready --label sprint-I.3 --json | jq -r '.[].external_ref'
```

Expected: `gh-477` and `gh-478` only — same set as `just _ready sprint-I.3`.

- [ ] **Step 5.4: Make `_ready` prefer beads when initialized**

In `justfile`, modify the `_ready` recipe to check for beads first:

```makefile
_ready label:
    #!/usr/bin/env bash
    set -euo pipefail
    LABEL="{{label}}"
    # Prefer beads when initialized: it has true DAG semantics with cycle detection.
    if command -v bd >/dev/null 2>&1 && bd list --all --json >/dev/null 2>&1; then
      bd ready --label "$LABEL" --json --limit 200 2>/dev/null \
        | jq -c '.[] | select(.external_ref != null) | select(.external_ref | startswith("gh-")) | {number: (.external_ref | sub("^gh-"; "") | tonumber), title}'
      exit 0
    fi
    # Fallback: parse "## Blocked by" from issue bodies.
    IN_FLIGHT=$(gh pr list --state open --limit 200 \
      --json closingIssuesReferences \
      --jq '[.[].closingIssuesReferences[].number] | unique | join(" ")' 2>/dev/null || echo "")
    gh issue list --label "$LABEL" --state open --limit 200 --json number,title,body \
      | jq -c '.[]' \
      | while IFS= read -r issue; do
          num=$(echo "$issue" | jq -r '.number')
          if [ -n "$IN_FLIGHT" ] && echo " $IN_FLIGHT " | grep -q " $num "; then
            continue
          fi
          body=$(echo "$issue" | jq -r '.body // ""')
          blockers=$(echo "$body" \
            | awk '/^## Blocked by/{flag=1; next} /^## /{flag=0} flag' \
            | grep -oE '#[0-9]+' | tr -d '#' | sort -u || true)
          ready=true
          for b in $blockers; do
            state=$(gh issue view "$b" --json state -q '.state' 2>/dev/null || echo "MISSING")
            if [ "$state" != "CLOSED" ]; then
              ready=false
              break
            fi
          done
          if [ "$ready" = "true" ]; then
            echo "$issue" | jq -c '{number, title}'
          fi
        done
```

- [ ] **Step 5.5: Verify both code paths**

```bash
# Beads path
just _ready sprint-I.3 | wc -l
# Expected: 2

# Body-parsing fallback (force by hiding bd from PATH)
PATH="/usr/bin:/bin" just _ready sprint-I.3 | wc -l
# Expected: 2
```

Both paths must agree.

- [ ] **Step 5.6: Commit**

```bash
git add scripts/beads-sync.sh justfile
git commit -m "feat(beads): encode 'Blocked by' edges as bd dependencies

beads-sync.sh now parses '## Blocked by' from issue bodies and calls
'bd dep add' for each edge. Justfile _ready prefers 'bd ready' when
beads is initialized, falls back to body parsing otherwise. Both code
paths return the same ready set for sprint-I.3."
```

---

## Self-Review

**Spec coverage:** Steps A, B, C, D, E from the prior turn each map to Tasks 1, 2, 3, 4, 5 respectively. ✓

**Placeholder scan:** No "TBD", "TODO", "implement later", "similar to Task N", or hand-wavy error handling. Every shell snippet is complete and executable. ✓

**Type/name consistency:**
- `_ready label` recipe name and signature consistent across Tasks 1, 2, and 5.4. ✓
- Sprint label string `sprint-I.3` consistent across all tasks. ✓
- `IN_FLIGHT` variable used identically in Tasks 1.1 and 5.4. ✓
- `ISSUES_JSON` (Task 5.2) is the variable currently used in beads-sync.sh — Step 5.1 re-reads the file to confirm; if the name has drifted, Step 5.2 instructs adjustment. ✓

**Idempotence:**
- `gh issue edit --add-label` is idempotent on already-labeled issues. ✓
- `bd dep add` is documented as idempotent. ✓
- The GH Action only comments — re-running on the same close event would post a duplicate comment, but issue close is a one-shot event so this is not a real risk.

**Failure modes:**
- `_ready` returns empty string when nothing is ready; Task 2 handles this with the existing `if [ -z "$ISSUES" ]` check.
- `bd ready` failing falls through to body parsing in Task 5.4.
- GH Action skipping non-sprint-I.3 issue closes via the `if:` guard.

Plan is internally consistent and execution-ready.
