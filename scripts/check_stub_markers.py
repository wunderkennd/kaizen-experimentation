#!/usr/bin/env python3
"""Stub-marker CI check.

Scans the PR diff for newly-added runtime-stub markers and fails if any
appear without an adjacent `stub-allow:` comment pointing at a tracking
issue or a cross-phase-artifacts row.

The check inspects *added lines only* (lines beginning with `+` in
`git diff base..head --unified=5`). Existing markers on `main` are
grandfathered: they only become a problem when the surrounding code is
edited, at which point the diff includes them and the allow comment is
required.

Markers detected (anchored on word boundaries where applicable):

    Rust:  todo!()  unimplemented!()  Status::unimplemented(
    Go:    codes.Unimplemented        status.Errorf(codes.Unimplemented
    Text:  NOT YET IMPLEMENTED        NOT IMPLEMENTED

Allow comment format (either of these is accepted, within 5 lines above
the marker line in the same hunk):

    // stub-allow: tracked-in #123
    // stub-allow: cross-phase-row "MigrateMetricDefinition"

Why this exists: ADR-026 Phase 3 shipped with `apply` and `shadow`
subcommands stubbed referencing a Phase C RPC that no phase produced.
See `docs/superpowers/templates/locked-plan-template.md` for the
process intervention this workflow enforces.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from typing import Iterable

# ---------------------------------------------------------------------------
# Patterns
# ---------------------------------------------------------------------------

MARKER_PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    ("Rust todo!()", re.compile(r"\btodo!\(\)")),
    ("Rust unimplemented!()", re.compile(r"\bunimplemented!\(\)")),
    ("tonic Status::unimplemented(", re.compile(r"Status::unimplemented\(")),
    ("Go codes.Unimplemented", re.compile(r"\bcodes\.Unimplemented\b")),
    ("Text: NOT YET IMPLEMENTED", re.compile(r"\bNOT\s+YET\s+IMPLEMENTED\b")),
    ("Text: NOT IMPLEMENTED", re.compile(r"\bNOT\s+IMPLEMENTED\b(?!\s*ED\b)")),
]

ALLOW_PATTERN = re.compile(
    r"""stub-allow:\s*
        (?: tracked-in\s+\#\d+        # tracked-in #123
          | cross-phase-row\s+"[^"]+" # cross-phase-row "Foo"
        )
    """,
    re.VERBOSE,
)

# Files we look at. Anything else is ignored.
SOURCE_SUFFIXES = (".rs", ".go", ".ts", ".tsx", ".js", ".jsx", ".py")

# Path fragments that mark a file as test/fixture/generated; exclude them.
EXCLUDE_FRAGMENTS = (
    "/test/",
    "/tests/",
    "/testdata/",
    "/__tests__/",
    "/__mocks__/",
    "/fixtures/",
    "/target/",
    "/node_modules/",
    "/dist/",
    "/build/",
    "/.next/",
    "/gen/",
    "/.git/",
    "/.codex/",
    "/.claire/",
    "/graphify-out/",
)
EXCLUDE_SUFFIXES = (
    "_test.rs",
    "_test.go",
    "_test.py",
    ".test.ts",
    ".test.tsx",
    ".test.js",
    ".spec.ts",
    ".spec.tsx",
    ".spec.js",
)

# Context window (lines above the marker) in which an allow comment must
# appear. Matches `--unified=N` passed to git diff.
ALLOW_CONTEXT_LINES = 5


# ---------------------------------------------------------------------------
# Diff parsing
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Violation:
    file_path: str
    line_no: int
    line: str
    marker_label: str

    def format(self) -> str:
        return (
            f"  {self.file_path}:{self.line_no}\n"
            f"      marker: {self.marker_label}\n"
            f"      line:   {self.line.strip()}\n"
        )


@dataclass
class Hunk:
    file_path: str
    head_start: int
    # (offset_from_head_start, is_added, line_text)
    lines: list[tuple[int, bool, str]]


def _included(path: str) -> bool:
    if not path.endswith(SOURCE_SUFFIXES):
        return False
    if any(path.endswith(s) for s in EXCLUDE_SUFFIXES):
        return False
    norm = "/" + path
    return not any(frag in norm for frag in EXCLUDE_FRAGMENTS)


def _run_git_diff(base: str, head: str) -> str:
    cmd = ["git", "diff", f"--unified={ALLOW_CONTEXT_LINES}", f"{base}..{head}"]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        sys.stderr.write(
            f"check_stub_markers: `git diff` exited {proc.returncode}\n"
            f"stderr: {proc.stderr.strip()}\n"
        )
        sys.exit(2)
    return proc.stdout


def _parse_diff(diff_text: str) -> Iterable[Hunk]:
    file_path: str | None = None
    head_start: int | None = None
    offset = 0
    buf: list[tuple[int, bool, str]] = []

    def flush() -> Hunk | None:
        if file_path and head_start is not None and buf:
            return Hunk(file_path=file_path, head_start=head_start, lines=list(buf))
        return None

    for raw in diff_text.splitlines():
        if raw.startswith("+++ b/"):
            h = flush()
            if h:
                yield h
            buf.clear()
            file_path = raw[6:]
            head_start = None
            offset = 0
            continue
        if raw.startswith("--- ") or raw.startswith("diff --git"):
            h = flush()
            if h:
                yield h
            buf.clear()
            head_start = None
            offset = 0
            continue
        if raw.startswith("@@"):
            h = flush()
            if h:
                yield h
            buf.clear()
            m = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@", raw)
            if not m:
                head_start = None
                continue
            head_start = int(m.group(1))
            offset = 0
            continue
        if head_start is None or file_path is None:
            continue
        if raw.startswith("+") and not raw.startswith("+++"):
            buf.append((offset, True, raw[1:]))
            offset += 1
        elif raw.startswith("-") and not raw.startswith("---"):
            # removed line; doesn't advance head offset, ignored for markers
            continue
        elif raw.startswith(" "):
            buf.append((offset, False, raw[1:]))
            offset += 1
        # Other lines (\ No newline at end of file, etc.) are skipped.

    h = flush()
    if h:
        yield h


# ---------------------------------------------------------------------------
# Marker detection
# ---------------------------------------------------------------------------


def _line_has_marker(text: str) -> bool:
    return any(pat.search(text) for _, pat in MARKER_PATTERNS)


def _find_violations(hunks: Iterable[Hunk]) -> list[Violation]:
    out: list[Violation] = []
    for hunk in hunks:
        if not _included(hunk.file_path):
            continue
        for idx, (offset, is_added, text) in enumerate(hunk.lines):
            if not is_added:
                continue
            for label, pat in MARKER_PATTERNS:
                if not pat.search(text):
                    continue
                # Walk backward up to ALLOW_CONTEXT_LINES. Accept the first
                # `stub-allow:` comment as covering this marker — BUT only if
                # no other marker line lies between the allow comment and the
                # current marker line. That prevents one allow comment from
                # silently absolving a stream of subsequent markers.
                allowed = False
                for back in range(idx - 1, max(-1, idx - ALLOW_CONTEXT_LINES - 1), -1):
                    prev_text = hunk.lines[back][2]
                    if _line_has_marker(prev_text):
                        # An intervening marker — stop searching; this one is
                        # unallowed even if an earlier allow exists farther up.
                        break
                    if ALLOW_PATTERN.search(prev_text):
                        allowed = True
                        break
                if allowed:
                    break
                out.append(
                    Violation(
                        file_path=hunk.file_path,
                        line_no=hunk.head_start + offset,
                        line=text,
                        marker_label=label,
                    )
                )
                break
    return out


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", required=True, help="Base ref/SHA (PR target).")
    ap.add_argument("--head", required=True, help="Head ref/SHA (PR source).")
    ap.add_argument(
        "--format",
        choices=("text", "github"),
        default="text",
        help="Output format. `github` emits GitHub Actions workflow commands.",
    )
    args = ap.parse_args()

    diff_text = _run_git_diff(args.base, args.head)
    violations = _find_violations(_parse_diff(diff_text))

    if not violations:
        if args.format == "github":
            print("::notice::No new stub markers introduced.")
        else:
            print("OK: no new stub markers in the diff.")
        return 0

    header = (
        f"Found {len(violations)} new stub marker(s) without `stub-allow:` comment.\n"
        f"\n"
        f"Each new runtime-stub marker (`todo!()`, `unimplemented!()`,\n"
        f"`Status::unimplemented(`, `codes.Unimplemented`, `NOT YET IMPLEMENTED`)\n"
        f"must have a `// stub-allow:` comment within {ALLOW_CONTEXT_LINES} lines above\n"
        f"naming either a tracking issue OR a cross-phase artifacts row.\n"
        f"\n"
        f"Accepted formats:\n"
        f"    // stub-allow: tracked-in #123\n"
        f"    // stub-allow: cross-phase-row \"MigrateMetricDefinition\"\n"
        f"\n"
        f"See docs/superpowers/templates/locked-plan-template.md for context.\n"
    )
    print(header)
    for v in violations:
        if args.format == "github":
            line = v.line.replace("%", "%25").replace("\n", "%0A").replace("\r", "%0D")
            print(
                f"::error file={v.file_path},line={v.line_no}::"
                f"New stub marker ({v.marker_label}) without `stub-allow:` "
                f"comment. Add `// stub-allow: tracked-in #NNN` or "
                f"`// stub-allow: cross-phase-row \"<artifact>\"` within "
                f"{ALLOW_CONTEXT_LINES} lines above. Line: {line}"
            )
        else:
            print(v.format())

    return 1


if __name__ == "__main__":
    sys.exit(main())
