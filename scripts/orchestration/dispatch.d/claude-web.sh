#!/usr/bin/env bash
# Executor adapter: Claude Code via the GitHub Action (@claude mention).
# Posts the rendered prompt as an @claude comment on the issue;
# .github/workflows/claude.yml (anthropics/claude-code-action) picks it up and
# runs the session in GitHub-hosted compute. No local daemon required.
#
# $1 = issue number; prompt on stdin.
set -euo pipefail
ISSUE="${1:?issue number required}"
{ printf '@claude '; cat; } | gh issue comment "$ISSUE" --body-file -
