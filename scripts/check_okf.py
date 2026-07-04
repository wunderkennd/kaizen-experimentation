#!/usr/bin/env python3
"""OKF v0.1 conformance check for the agent registry bundle.

Validates docs/agents/registry/ (or a directory passed as argv[1]) against the
Open Knowledge Format v0.1 conformance criteria
(https://github.com/GoogleCloudPlatform/knowledge-catalog/tree/main/okf):

  1. Every non-reserved .md file contains a parseable YAML frontmatter block.
  2. Every frontmatter block contains a non-empty `type` field.
  3. Reserved filenames follow their prescribed structure:
     - index.md carries no frontmatter, except the bundle-root index.md, which
       may carry ONLY `okf_version`.
     - log.md is a flat list of ISO-8601 date headings (## YYYY-MM-DD),
       newest-first.

Additionally WARNS (never fails) on bundle-absolute links (/path.md) whose
target is missing — OKF consumers must tolerate broken links, so we surface
them without gating.

Exit codes: 0 conformant, 1 violations found, 2 usage/IO error.
Used by `just check-registry` and .github/workflows/registry-conformance.yml.
"""

import re
import sys
from pathlib import Path

RESERVED = {"index.md", "log.md"}
FRONTMATTER_RE = re.compile(r"\A---\n(.*?)\n---\n", re.DOTALL)
DATE_HEADING_RE = re.compile(r"^## (\d{4}-\d{2}-\d{2})\s*$", re.MULTILINE)
ABS_LINK_RE = re.compile(r"\]\((/[^)#\s]+\.md)")


def parse_frontmatter(text):
    """Return (dict-or-None, raw-block-or-None). Prefers PyYAML, falls back to
    a line parser sufficient for key detection."""
    m = FRONTMATTER_RE.match(text)
    if not m:
        return None, None
    raw = m.group(1)
    try:
        import yaml  # type: ignore

        data = yaml.safe_load(raw)
        return (data if isinstance(data, dict) else {}), raw
    except Exception:
        data = {}
        for line in raw.splitlines():
            km = re.match(r"^([A-Za-z0-9_]+):\s*(.*)$", line)
            if km:
                data[km.group(1)] = km.group(2).strip().strip("\"'")
        return data, raw


def check_bundle(root: Path):
    errors, warnings = [], []
    md_files = sorted(root.rglob("*.md"))
    if not md_files:
        errors.append(f"{root}: no .md files found — not a bundle?")
        return errors, warnings

    for path in md_files:
        rel = path.relative_to(root)
        text = path.read_text(encoding="utf-8")
        fm, raw = parse_frontmatter(text)

        if path.name in RESERVED:
            if path.name == "index.md":
                is_root = path.parent == root
                if fm is not None:
                    keys = set(fm.keys())
                    if not is_root:
                        errors.append(f"{rel}: non-root index.md must not carry frontmatter")
                    elif keys - {"okf_version"}:
                        errors.append(
                            f"{rel}: root index.md frontmatter may only carry okf_version "
                            f"(found: {', '.join(sorted(keys))})"
                        )
            else:  # log.md
                if fm is not None:
                    errors.append(f"{rel}: log.md must not carry frontmatter")
                dates = DATE_HEADING_RE.findall(text)
                if not dates:
                    errors.append(f"{rel}: log.md needs at least one '## YYYY-MM-DD' heading")
                elif dates != sorted(dates, reverse=True):
                    errors.append(f"{rel}: log.md date headings must be newest-first")
            continue

        # Rule 1: parseable frontmatter present
        if fm is None:
            errors.append(f"{rel}: missing YAML frontmatter block (--- ... ---)")
            continue
        # Rule 2: non-empty type
        type_val = fm.get("type")
        if not (isinstance(type_val, str) and type_val.strip()):
            errors.append(f"{rel}: frontmatter missing non-empty 'type' field")

        # Advisory: bundle-absolute link targets
        for link in ABS_LINK_RE.findall(text):
            if not (root / link.lstrip("/")).exists():
                warnings.append(f"{rel}: bundle-absolute link target missing: {link}")

    return errors, warnings


def main():
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("docs/agents/registry")
    if not root.is_dir():
        print(f"error: {root} is not a directory", file=sys.stderr)
        return 2
    errors, warnings = check_bundle(root)
    for w in warnings:
        print(f"WARN  {w}")
    for e in errors:
        print(f"ERROR {e}")
    n = len([p for p in root.rglob('*.md') if p.name not in RESERVED])
    if errors:
        print(f"\n✗ {root}: {len(errors)} violation(s) across bundle ({n} concepts)")
        return 1
    print(f"✓ {root}: OKF v0.1 conformant ({n} concepts, {len(warnings)} link warning(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
