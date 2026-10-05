#!/usr/bin/env bash
# Vendor kaizen-rosetta's protos into proto/ at the commit pinned in
# proto/rosetta.lock.json. Rosetta is the source of truth for these files;
# never hand-edit them here — change rosetta, bump the pin, re-run this.
#
# Usage:
#   scripts/sync_rosetta_protos.sh            # rewrite proto/<paths> from the pin
#   scripts/sync_rosetta_protos.sh --check    # exit 1 if proto/<paths> drifted
#
# ROSETTA_REPO=/path/to/kaizen-rosetta reuses a local checkout (it must
# contain the pinned commit); otherwise the pinned commit is fetched from
# GitHub into a temp dir.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lock="$root/proto/rosetta.lock.json"
mode="${1:-sync}"

repo="$(jq -r .repository "$lock")"
commit="$(jq -r .commit "$lock")"
mapfile -t paths < <(jq -r '.paths[]' "$lock")

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

src="${ROSETTA_REPO:-}"
if [ -z "$src" ]; then
  src="$tmp/rosetta"
  git init -q "$src"
  git -C "$src" fetch -q --depth 1 "https://github.com/$repo.git" "$commit"
fi
git -C "$src" archive "$commit" -- $(printf 'proto/%s ' "${paths[@]}") | tar -x -C "$tmp"

status=0
for p in "${paths[@]}"; do
  if [ "$mode" = "--check" ]; then
    if ! diff -r "$tmp/proto/$p" "$root/proto/$p"; then
      status=1
    fi
  else
    rm -rf "${root:?}/proto/$p"
    mkdir -p "$(dirname "$root/proto/$p")"
    cp -R "$tmp/proto/$p" "$root/proto/$p"
  fi
done

if [ "$mode" = "--check" ]; then
  if [ "$status" -ne 0 ]; then
    echo "✗ proto/ differs from $repo@$commit — run 'just sync-rosetta-protos' (or fix rosetta and bump proto/rosetta.lock.json)" >&2
    exit 1
  fi
  echo "✓ proto/ matches $repo@$commit"
else
  echo "Vendored ${paths[*]} from $repo@$commit"
fi
