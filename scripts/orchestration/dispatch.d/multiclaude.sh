#!/usr/bin/env bash
# Executor adapter: multiclaude worker daemon (today's default overnight path).
# Reads the rendered task prompt on stdin; $1 = issue number (unused here —
# the prompt carries all context).
set -euo pipefail
multiclaude worker create "$(cat)"
