#!/usr/bin/env python3
"""Generate agent-context views from the OKF registry (#682, proposal §7 R3).

Canonical source: docs/agents/registry/*.md (frontmatter identity + charter).
Generated views (all marked, all overwritten wholesale):

  1. <owned_dir>/AGENTS.md      — one per *directory* in owned_paths
                                   (nearest-file-wins; honored by Jules, Devin,
                                   Codex, Cursor, Copilot). File-grained
                                   owned_paths (infra agents own individual
                                   .go files) are NOT given per-file views;
                                   the root table carries them.
  2. AGENTS.md (repo root)       — vendor-neutral anchor: pointers to CLAUDE.md
                                   and the registry, plus the full ownership
                                   table (directories AND files).
  3. .multiclaude/agents/<id>-*.md — regenerated executor views (replaces the
                                   hand-maintained copies and their stale
                                   Phase-5 task lists; registry is canonical).

Bundle-absolute links (](/agent-N.md)) are rewritten to full GitHub URLs so
views resolve from any depth.

Usage:
  python3 scripts/gen_agents.py            # write views into the repo
  python3 scripts/gen_agents.py --check    # exit 1 if any view is stale/missing
  python3 scripts/gen_agents.py --root D   # operate on tree D (tests)

Exit codes: 0 ok / views written; 1 --check found drift; 2 usage or registry error.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_okf import parse_frontmatter  # noqa: E402

REPO_URL = "https://github.com/wunderkennd/kaizen-experimentation"
BANNER = (
    "<!-- GENERATED from {src} by scripts/gen_agents.py — DO NOT EDIT.\n"
    "     Edit the registry concept, then run `just gen-agents`. -->\n"
)


def load_registry(root: Path):
    reg = root / "docs" / "agents" / "registry"
    if not reg.is_dir():
        print(f"gen-agents: registry not found at {reg}", file=sys.stderr)
        sys.exit(2)
    agents = []
    for f in sorted(reg.glob("*.md")):
        if f.name in ("index.md", "log.md"):
            continue
        text = f.read_text(encoding="utf-8")
        fm, raw = parse_frontmatter(text)
        if not fm or "id" not in fm:
            print(f"gen-agents: {f} has no parseable frontmatter id", file=sys.stderr)
            sys.exit(2)
        # body = everything after the closing frontmatter fence
        end = text.find("\n---\n", 4)
        body = text[end + 5 :] if end != -1 else ""
        agents.append({"fm": fm, "body": body.strip(), "src": f})
    if not agents:
        print("gen-agents: registry is empty", file=sys.stderr)
        sys.exit(2)
    return agents


def rewrite_links(text: str) -> str:
    return text.replace("](/", f"]({REPO_URL}/blob/main/docs/agents/registry/")


def identity_block(fm) -> str:
    lines = []
    if fm.get("language"):
        lines.append(f"- **Language**: {fm['language']}")
    ports = fm.get("ports") or []
    if ports:
        lines.append(f"- **Ports**: {', '.join(str(p) for p in ports)}")
    owned = fm.get("owned_paths") or []
    if owned:
        lines.append(f"- **Owned paths**: {', '.join('`' + p + '`' for p in owned)}")
    deps = fm.get("depends_on") or []
    if deps:
        lines.append(f"- **Depends on**: {', '.join(deps)}")
    lines.append(
        f"- **Work queue**: `gh issue list --label \"{fm['label']}\" --state open` "
        f"(claim protocol: `scripts/orchestration/README.md`)"
    )
    return "\n".join(lines)


def module_view(a) -> str:
    fm = a["fm"]
    src = f"docs/agents/registry/{a['src'].name}"
    return (
        BANNER.format(src=src)
        + f"# {fm.get('title', fm['id'])}\n\n"
        + f"{fm.get('description', '').strip()}\n\n"
        + identity_block(fm)
        + "\n\n"
        + f"Canonical identity & charter: [`{src}`]({REPO_URL}/blob/main/{src}) · "
        + f"Repo context anchor: [`CLAUDE.md`]({REPO_URL}/blob/main/CLAUDE.md)\n\n"
        + rewrite_links(a["body"])
        + "\n"
    )


def multiclaude_view(a) -> str:
    # Same content as the module view — one source, one shape; the executor
    # preamble (how sessions start) lives in the H1 dispatch prompt, not here.
    return module_view(a)


def root_view(agents) -> str:
    rows = []
    for a in agents:
        fm = a["fm"]
        for p in fm.get("owned_paths") or []:
            rows.append((p, fm["id"], fm.get("title", fm["id"])))
    rows.sort()
    table = "\n".join(f"| `{p}` | {i} | {t} |" for p, i, t in rows)
    return (
        BANNER.format(src="docs/agents/registry/ (the full bundle)")
        + "# Kaizen — Agent Context (vendor-neutral anchor)\n\n"
        + "This file exists for tools that discover context via the "
        + "[agents.md](https://agents.md) convention (Jules, Devin, Codex, Cursor, "
        + "Copilot, …). It is a **generated view** — two sources outrank it:\n\n"
        + f"1. [`CLAUDE.md`]({REPO_URL}/blob/main/CLAUDE.md) — the repo-wide context "
        + "anchor (architecture, rules, commands, work tracking). **Read it first.**\n"
        + f"2. [`docs/agents/registry/`]({REPO_URL}/tree/main/docs/agents/registry) — "
        + "canonical per-agent identity + charters (OKF v0.1 bundle).\n\n"
        + "Directory-scoped `AGENTS.md` views are generated into each owned "
        + "directory (nearest-file-wins). Ownership map (directories and files):\n\n"
        + "| Path | Agent | Charter |\n| --- | --- | --- |\n"
        + table
        + "\n\nDispatch, claims, and readiness: `scripts/orchestration/README.md`. "
        + "Executor lanes are pluggable (`dispatch.d/`); this file names no vendor.\n"
    )


def mc_filename(root: Path, agent_id: str) -> Path:
    d = root / ".multiclaude" / "agents"
    hits = sorted(d.glob(f"{agent_id}-*.md"))
    return hits[0] if hits else d / f"{agent_id}.md"


def render_all(root: Path, agents):
    out = {}
    for a in agents:
        for p in a["fm"].get("owned_paths") or []:
            if p.endswith("/"):
                out[root / p / "AGENTS.md"] = module_view(a)
        out[mc_filename(root, a["fm"]["id"])] = multiclaude_view(a)
    out[root / "AGENTS.md"] = root_view(agents)
    return out


def main():
    args = sys.argv[1:]
    check = "--check" in args
    root = Path(__file__).resolve().parent.parent
    if "--root" in args:
        root = Path(args[args.index("--root") + 1]).resolve()
    agents = load_registry(root)
    out = render_all(root, agents)

    drift = []
    for path, content in sorted(out.items()):
        current = path.read_text(encoding="utf-8") if path.exists() else None
        if current == content:
            continue
        if check:
            drift.append(path)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
            print(f"wrote {path.relative_to(root)}")
    if check:
        if drift:
            print("gen-agents --check: STALE views (run `just gen-agents`):")
            for p in drift:
                print(f"  {p.relative_to(root)}")
            sys.exit(1)
        print(f"gen-agents --check: clean ({len(out)} views current)")
    else:
        print(f"gen-agents: {len(out)} views considered")


if __name__ == "__main__":
    main()
