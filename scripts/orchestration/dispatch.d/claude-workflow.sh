#!/usr/bin/env bash
# Executor adapter: Claude Code worker on GitHub Actions compute (H4, #716).
# Launches .github/workflows/claude-worker.yml via workflow_dispatch — the
# default GITHUB_TOKEN can do this (probe #713 leg 1), and the worker file
# must be registered on the default branch (probe #713's 404 finding).
# $1 = issue number; rendered prompt on stdin.
#
# Plan L8: workflow_dispatch payloads cap at ~64KB — fail loudly past
# 60,000 chars rather than truncate a worker's instructions.
set -euo pipefail
ISSUE="${1:?issue number required}"
PROMPT=$(cat)
if [ "${#PROMPT}" -gt 60000 ]; then
  echo "claude-workflow adapter: prompt is ${#PROMPT} chars — exceeds the 60000 budget (plan 2026-07-06 L8); refusing to truncate" >&2
  exit 1
fi
gh workflow run claude-worker.yml -f issue="$ISSUE" -f prompt="$PROMPT"
