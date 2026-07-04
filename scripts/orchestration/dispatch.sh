#!/usr/bin/env bash
# Executor-agnostic dispatch (harness H1, #679):
#
#   dispatch.sh <issue-number> [executor]
#
# claim → render task prompt from the Issue (init vs. resume) → hand to the
# executor adapter → release the claim if the adapter fails to launch.
#
# Executors are pluggable: dispatch.d/<executor>.sh reads the rendered prompt
# on stdin and receives the issue number as $1. Ships with: multiclaude
# (worker daemon, today's default), claude-web (posts an @claude comment —
# .github/workflows/claude.yml picks it up), jules (Google Jules cloud VM).
#
# Env knobs:
#   ORCH_DEFAULT_EXECUTOR  default executor when $2 omitted (multiclaude)
#   ORCH_BRANCH_HINT       branch-naming hint embedded in the prompt
#   ORCH_CLAIM_TTL_HOURS   lease length (default 24)
#
# Exit codes: 0 dispatched; 3 already claimed; 1 adapter/usage failure.

set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

ISSUE="${1:?usage: dispatch.sh <issue-number> [executor]}"
EXECUTOR="${2:-${ORCH_DEFAULT_EXECUTOR:-multiclaude}}"
ADAPTER_DIR="${ORCH_ADAPTER_DIR:-$HERE/dispatch.d}"
ADAPTER="$ADAPTER_DIR/$EXECUTOR.sh"
BRANCH_HINT="${ORCH_BRANCH_HINT:-use the agent-N/feat/adr-XXX-description naming convention (see CLAUDE.md Branch-naming)}"

if [ ! -x "$ADAPTER" ] && [ ! -f "$ADAPTER" ]; then
  echo "dispatch: unknown executor '$EXECUTOR' (no $ADAPTER)" >&2
  echo "available: $(ls "$ADAPTER_DIR" | sed 's/\.sh$//' | tr '\n' ' ')" >&2
  exit 1
fi

WORKER="${EXECUTOR}-$(hostname -s 2>/dev/null || echo host)-$$-$(date +%s)"

bash "$HERE/claims.sh" take "$ISSUE" "$EXECUTOR" "$WORKER" || exit $?

TITLE=$(gh issue view "$ISSUE" --json title -q '.title')
BODY=$(gh issue view "$ISSUE" --json body -q '.body // ""')
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner' 2>/dev/null || echo "this repository")

# Resume detection: a prior worker announces its branch with a
# `progress-branch: <name>` comment after its first push.
RESUME_BRANCH=$(gh issue view "$ISSUE" --json comments --jq '.comments[].body' 2>/dev/null \
  | grep -E '^progress-branch: ' | tail -1 | sed 's/^progress-branch: //' || true)

if [ -n "$RESUME_BRANCH" ]; then
  MODE_BLOCK=$(cat <<EOF
MODE: RESUME — a previous session already worked this issue on branch \`$RESUME_BRANCH\`.
2. Fetch and check out \`$RESUME_BRANCH\`. Read \`progress.log.md\` at the branch root
   and \`git log --oneline -15\` to learn exactly where work stopped. Do NOT start a
   new branch and do NOT open a duplicate PR — continue the existing work.
EOF
)
else
  MODE_BLOCK=$(cat <<EOF
MODE: INIT — first session on this issue.
2. Create a fresh branch: $BRANCH_HINT. Create \`progress.log.md\` at the branch root
   (OKF log.md conventions: \`## YYYY-MM-DD\` headings, newest first, append-only) and
   record what you plan to do this session.
EOF
)
fi

PROMPT=$(cat <<EOF
You are a harness worker dispatched against GitHub Issue #$ISSUE of $REPO.

Issue title: $TITLE

== Startup ritual — complete IN ORDER before any new work ==
1. Sync main and read the repo context anchor (CLAUDE.md). Agent identity and
   ownership live in docs/agents/registry/ if unclear.
$MODE_BLOCK
3. Verify the baseline is green BEFORE new work: run the narrowest relevant test
   target from CLAUDE.md's Testing Commands for the paths you will touch. If the
   baseline is red, fix or report that first — starting a feature on a broken
   baseline makes it worse. Record the baseline result in progress.log.md.

== Task (from the Issue body) ==
$BODY

== Working rules ==
- One unit of work per session. Leave a merge-ready state: no half-implemented
  features, code orderly and documented, tests green.
- Commit incrementally with descriptive messages.
- progress.log.md is append-only status: flip statuses and add dated entries;
  it is unacceptable to remove or edit existing spec or test content.
- After your FIRST push, post an issue comment that is exactly:
  progress-branch: <your-branch-name>
- Your PR description must include 'Closes #$ISSUE'. Mark the PR ready for
  review (not draft) when the work is complete — or, if you opened a draft,
  add the 'ready' label as your final step.
- If reviewers leave feedback after that, address every thread — push the fix
  or reply with why not, then resolve the conversation. Merge is gated on zero
  unresolved threads and no standing changes-requested (Review gate check).
EOF
)

echo "=== Dispatching issue #$ISSUE via $EXECUTOR (worker $WORKER) ==="
if printf '%s\n' "$PROMPT" | bash "$ADAPTER" "$ISSUE"; then
  echo "✓ dispatched #$ISSUE → $EXECUTOR"
else
  rc=$?
  echo "✗ adapter '$EXECUTOR' failed (rc=$rc) — releasing claim" >&2
  bash "$HERE/claims.sh" release "$ISSUE" "$WORKER" || true
  exit 1
fi
