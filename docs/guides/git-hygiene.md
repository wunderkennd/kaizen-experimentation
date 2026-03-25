# Git Hygiene

What belongs in git, what doesn't, and how to configure `.gitignore` and `.gitattributes`.

## Principle

Track **source code and shared configuration**. Ignore **build artifacts, agent runtime state, and user-local overrides**. Track work progress in **GitHub Issues**, not in-repo files.

## Files That Must Be Tracked

### Source and Configuration
- All source code (`crates/`, `services/`, `ui/src/`, `proto/`, `sdks/`)
- `Cargo.toml`, `Cargo.lock` — Rust workspace and lockfile
- `go.mod`, `go.sum`, `go.work`, `go.work.sum` — Go modules and workspace checksums
- `package.json`, `package-lock.json` — Node dependencies
- `buf.yaml`, `buf.gen.yaml` — Proto toolchain config
- `justfile` — Task runner recipes
- `docker-compose.yml`, `docker-compose.monitoring.yml`, `docker-compose.test.yml`
- `.github/workflows/` — CI/CD pipelines and Jules automation
- `.github/ISSUE_TEMPLATE/` — Issue templates for ADR work
- `sql/migrations/` — PostgreSQL DDL
- `delta/` — Delta Lake table schemas
- `test-vectors/` — Hash parity test vectors
- `scripts/` — Bootstrap and utility scripts (e.g., `create-phase5-issues.sh`)

### Agent Configuration (shared)
- `.claude/settings.json` — Project-level Claude Code settings
- `.claude/agents/*.md` — Subagent definitions (e.g., pr-triage)
- `.multiclaude/config.json` — Multiclaude repo settings
- `.multiclaude/agents/*.md` — The 7 Kaizen agent definitions

### Documentation
- `CLAUDE.md`, `AGENTS.md`, `README.md`, `CONTRIBUTING.md`
- `docs/` — All documentation, ADRs, coordination files, guides

## Files That Must NOT Be Tracked

### Build Artifacts
- `target/` — Rust build output
- `node_modules/` — npm packages
- `.next/` — Next.js build cache
- `dist/` — TypeScript build output
- `ui/tsconfig.tsbuildinfo` — TypeScript incremental compilation cache

### Agent Runtime State
- `.Jules/` — Jules session state
- `.claude/settings.local.json` — User-specific Claude Code overrides
- `.claude/worktrees/`, `.claude/teams/`, `.claude/tasks/`, `.claude/credentials/`, `.claude/cache/`, `.claude/*.log`
- `.multiclaude/state/`, `.multiclaude/messages/`, `.multiclaude/locks/`, `.multiclaude/worktrees/`, `.multiclaude/*.pid`, `.multiclaude/*.log`

### Work Tracking State
- `docs/coordination/status/` — Legacy per-agent status files (replaced by GitHub Issues)

### Environment and Secrets
- `.env`, `.env.local`, `.env.*.local`
- `*.pem`, `*.key`

## .gitignore

```gitignore
# Build artifacts
target/
node_modules/
.next/
dist/
ui/tsconfig.tsbuildinfo

# Agent runtime state
.Jules/
.claude/settings.local.json
.claude/worktrees/
.claude/teams/
.claude/tasks/
.claude/credentials/
.claude/statsig/
.claude/cache/
.claude/*.log
.multiclaude/state/
.multiclaude/messages/
.multiclaude/locks/
.multiclaude/worktrees/
.multiclaude/*.pid
.multiclaude/*.log

# Environment and secrets
.env
.env.local
.env.*.local
*.pem
*.key
```

## .gitattributes

```gitattributes
# Lock files: mark as generated
Cargo.lock linguist-generated=true
go.sum linguist-generated=true
go.work.sum linguist-generated=true
package-lock.json linguist-generated=true

# Proto generated code: mark as generated
crates/experimentation-proto/src/**/*.rs linguist-generated=true
```

Note: The `theirs-status` merge driver for status files is no longer needed since work tracking moved to GitHub Issues.

## Recovering from Accidental Tracking

```bash
git rm --cached <file>
grep -q "<pattern>" .gitignore || echo "<pattern>" >> .gitignore
git commit -m "chore: stop tracking <file> (build artifact)"
```

If multiple branches have the file, batch rebase them — see `docs/guides/merge-conflict-resolution.md`.

## Verification

Run periodically:
```bash
# Check for accidentally tracked artifacts
git ls-files | grep -E '\.tsbuildinfo|\.next/|node_modules/|target/|dist/|\.Jules/'

# Check for large files
git ls-files | xargs ls -la 2>/dev/null | awk '$5 > 1000000 {print $5, $9}' | sort -rn

# Check for secrets
git ls-files | xargs grep -l "PRIVATE KEY\|API_KEY\|SECRET" 2>/dev/null
```
