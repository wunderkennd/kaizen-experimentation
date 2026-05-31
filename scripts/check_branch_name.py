#!/usr/bin/env python3
"""
Validate a branch name against the allowlist in .github/branch-naming.yml.

Single source of truth shared by:
  - `just check-branch-name`               (local pre-push CLI)
  - `.github/workflows/branch-naming.yml`  (advisory PR comment)

Usage:
    check_branch_name.py [BRANCH]
        BRANCH defaults to the current git HEAD's branch name.

Exit codes:
    0 — branch name matches an allowed pattern
    1 — branch name matches no allowed pattern (and suggestions are printed)
    2 — invocation error (missing config, bad arguments, etc.)

Output:
    On match:    `✓ branch 'X' matches: <pattern>` to stdout.
    On no match: diagnosis + suggested renames + rename commands to stdout.

When `--format=github` is passed, also emits GitHub Actions output variables
(matched, matched_pattern, branch, suggestions) for workflow consumption.
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    print("✗ check_branch_name.py requires PyYAML (pip install pyyaml)", file=sys.stderr)
    sys.exit(2)


REPO_ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = REPO_ROOT / ".github" / "branch-naming.yml"


def current_branch() -> str:
    """Return the current git branch name (e.g. 'agent-3/feat/foo')."""
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"],
            text=True,
        ).strip()
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print(f"✗ failed to read current branch: {e}", file=sys.stderr)
        sys.exit(2)


def load_patterns() -> list[str]:
    """Read the allowlist regexes from `.github/branch-naming.yml`."""
    if not CONFIG_PATH.exists():
        print(f"✗ allowlist config missing: {CONFIG_PATH}", file=sys.stderr)
        sys.exit(2)
    cfg = yaml.safe_load(CONFIG_PATH.read_text())
    patterns = cfg.get("allowed_patterns", []) if isinstance(cfg, dict) else []
    if not patterns:
        print(f"✗ no allowed_patterns in {CONFIG_PATH}", file=sys.stderr)
        sys.exit(2)
    return list(patterns)


def suggestion_slug(branch: str) -> str:
    """Build a friendly slug from the original branch name for suggestions."""
    return re.sub(r"[^a-z0-9-]+", "-", branch.lower()).strip("-")[:60] or "work"


def suggestions(branch: str) -> list[str]:
    slug = suggestion_slug(branch)
    return [
        f"agent-N/feat/adr-XXX-{slug}   (replace N + XXX with the owning agent + ADR)",
        f"infra-N/feat/{slug}           (if this is Pulumi / infra work)",
        f"chore/{slug}                  (if this is repo-wide hygiene)",
    ]


def emit_github_outputs(
    branch: str,
    matched: str | None,
    suggestions_list: list[str],
) -> None:
    """Append outputs for the GitHub Actions workflow consumer."""
    out_path = os.environ.get("GITHUB_OUTPUT")
    if not out_path:
        return  # not running under Actions
    with open(out_path, "a") as f:
        f.write(f"branch={branch}\n")
        f.write(f"matched={'true' if matched else 'false'}\n")
        if matched:
            f.write(f"matched_pattern={matched}\n")
        else:
            f.write("suggestions<<SUGEOF\n")
            for s in suggestions_list:
                f.write(f"- `{s.split('  ')[0]}` {s[len(s.split('  ')[0]):].strip()}\n")
            f.write("SUGEOF\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("branch", nargs="?", help="branch name (default: current HEAD)")
    parser.add_argument(
        "--format",
        choices=["human", "github"],
        default="human",
        help="output mode (human: stdout messages; github: also write Actions outputs)",
    )
    args = parser.parse_args()

    branch = args.branch if args.branch else current_branch()
    patterns = load_patterns()

    matched = next((p for p in patterns if re.fullmatch(p, branch)), None)
    sugg = suggestions(branch) if not matched else []

    if args.format == "github":
        emit_github_outputs(branch, matched, sugg)

    if matched:
        print(f"✓ branch {branch!r} matches: {matched}")
        return 0

    print(f"✗ branch {branch!r} matches no allowed pattern in .github/branch-naming.yml")
    print()
    print("Allowed pattern families:")
    for p in patterns:
        print(f"  - {p}")
    print()
    print("Suggested renames:")
    for s in sugg:
        print(f"  {s}")
    print()
    print("To rename locally before push:")
    print("  git branch -m <new-name>")
    print("  git push origin -u <new-name>")
    print(f"  git push origin --delete {branch}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
