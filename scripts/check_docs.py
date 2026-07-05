#!/usr/bin/env python3
"""Advisory lints for delivery-lifecycle documents (H7 #699, PR-3).

Extends the check_okf.py pattern to the artifact conventions in
docs/guides/delivery-lifecycle.md:

  ADRs   docs/adrs/NNN-*.md          title `# ADR-NNN:`, Status + Date markers
  Plans  docs/superpowers/plans/     dated filename; plans dated on/after the
                                     locked-plan v2 cutover (2026-07-05) must
                                     carry the v2 sections (Platform
                                     assumptions & probes, Locks, Plan-review
                                     link) and — when multi-phase — the
                                     Cross-phase artifacts table. Older plans
                                     get the same checks as warnings
                                     (grandfathered).
  Specs  docs/superpowers/specs/     dated filename
  PRDs   docs/prds/                  frontmatter `type: PRD`; exactly ONE
                                     Primary metric bullet (the Goal rule)
  Templates docs/templates/          frontmatter parses and carries `type`

Ratchet (verify-then-require): by default warnings do not fail the run —
only structural errors on post-cutover artifacts do. DOCS_LINT_STRICT=1
escalates warnings to errors; promotion to a required check happens only
after a clean window, per the lifecycle map.

Exit codes: 0 clean (or warnings in default mode) · 1 errors · 2 usage.
"""

from __future__ import annotations

import os
import re
import sys
from datetime import date
from pathlib import Path

V2_CUTOVER = date(2026, 7, 5)  # locked-plan template v2 (H7 PR-2)

errors: list[str] = []
warnings: list[str] = []


def err(path: Path, msg: str) -> None:
    errors.append(f"{path}: {msg}")


def warn(path: Path, msg: str) -> None:
    warnings.append(f"{path}: {msg}")


def head(text: str, lines: int = 15) -> str:
    return "\n".join(text.splitlines()[:lines])


def dated_name(path: Path):
    m = re.match(r"^(\d{4})-(\d{2})-(\d{2})-.+\.md$", path.name)
    if not m:
        return None
    try:
        return date(*map(int, m.groups()))
    except ValueError:
        return None


def frontmatter(text: str):
    """Parse a leading YAML frontmatter block; None if absent."""
    m = re.match(r"^---\n(.*?)\n---\n", text, re.DOTALL)
    if not m:
        return None
    try:
        import yaml  # same soft dependency posture as check_okf.py

        return yaml.safe_load(m.group(1)) or {}
    except ImportError:
        # Regex fallback: top-level `key: value` pairs only.
        out = {}
        for line in m.group(1).splitlines():
            km = re.match(r"^([A-Za-z_][\w-]*):\s*(.*)$", line)
            if km:
                out[km.group(1)] = km.group(2).strip().strip("\"'")
        return out
    except Exception as e:  # yaml parse error
        return e


def check_adrs(root: Path) -> None:
    for path in sorted((root / "docs" / "adrs").glob("[0-9][0-9][0-9]-*.md")):
        text = path.read_text(encoding="utf-8")
        h = head(text)
        if not re.search(r"^# ADR-\d+", h, re.M):
            warn(path, "first heading is not '# ADR-NNN: …'")
        if "**Status**" not in h:
            warn(path, "no **Status** marker in the first 15 lines")
        if "**Date**" not in h:
            warn(path, "no **Date** marker in the first 15 lines")


def check_plans(root: Path) -> None:
    for path in sorted((root / "docs" / "superpowers" / "plans").glob("*.md")):
        text = path.read_text(encoding="utf-8")
        d = dated_name(path)
        if d is None:
            warn(path, "filename is not YYYY-MM-DD-<slug>.md")
        post_v2 = d is not None and d >= V2_CUTOVER
        report = err if post_v2 else warn
        tag = "" if post_v2 else " (grandfathered pre-v2 — warning only)"

        multi_phase = len(set(re.findall(r"^## Phase ([A-Z])\b", text, re.M))) > 1
        if multi_phase and "## Cross-phase artifacts" not in text:
            report(path, "multi-phase plan without a '## Cross-phase artifacts' table" + tag)
        if "## Locks" not in text:
            report(path, "no '## Locks' section" + tag)
        if "## Platform assumptions" not in text:
            report(path, "no '## Platform assumptions & probes' section (v2)" + tag)
        if "**Plan-review:**" not in text:
            report(path, "no '**Plan-review:**' link in the status block (v2)" + tag)


def check_specs(root: Path) -> None:
    for path in sorted((root / "docs" / "superpowers" / "specs").glob("*.md")):
        if dated_name(path) is None:
            warn(path, "filename is not YYYY-MM-DD-<slug>.md")


def check_prds(root: Path) -> None:
    prds = root / "docs" / "prds"
    if not prds.is_dir():
        return
    for path in sorted(prds.glob("*.md")):
        text = path.read_text(encoding="utf-8")
        fm = frontmatter(text)
        if fm is None:
            err(path, "PRD has no YAML frontmatter (see docs/templates/prd-template.md)")
            continue
        if isinstance(fm, Exception):
            err(path, f"frontmatter does not parse: {fm}")
            continue
        if fm.get("type") != "PRD":
            err(path, "frontmatter `type` must be 'PRD'")
        metrics = re.findall(r"^\s*-\s*\*\*Primary metric\*\*", text, re.M)
        if len(metrics) != 1:
            err(path, f"exactly ONE '**Primary metric**' bullet required (found {len(metrics)}) — the Goal rule")
        if dated_name(path) is None:
            warn(path, "filename is not YYYY-MM-DD-<slug>.md")


def check_templates(root: Path) -> None:
    tdir = root / "docs" / "templates"
    if not tdir.is_dir():
        return
    for path in sorted(tdir.glob("*-template.md")):
        text = path.read_text(encoding="utf-8")
        fm = frontmatter(text)
        if isinstance(fm, Exception):
            err(path, f"template frontmatter does not parse: {fm}")
        elif fm is not None and "type" not in fm:
            warn(path, "template frontmatter has no `type` key (OKF: type is the one required key)")


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
    if not (root / "docs").is_dir():
        print(f"usage: {sys.argv[0]} [repo-root] — no docs/ under {root}", file=sys.stderr)
        return 2

    check_adrs(root)
    check_plans(root)
    check_specs(root)
    check_prds(root)
    check_templates(root)

    strict = os.environ.get("DOCS_LINT_STRICT") == "1"
    for w in warnings:
        print(f"warning: {w}")
    for e in errors:
        print(f"ERROR: {e}")
    print(
        f"check_docs: {len(errors)} error(s), {len(warnings)} warning(s)"
        f"{' [strict]' if strict else ''}"
    )
    if errors or (strict and warnings):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
