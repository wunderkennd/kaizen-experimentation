#!/usr/bin/env bash
# Executor adapter: Google Jules (async cloud VM).
# $1 = issue number (context only); prompt on stdin.
set -euo pipefail
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')
jules remote new --repo "$REPO" --session "$(cat)"
