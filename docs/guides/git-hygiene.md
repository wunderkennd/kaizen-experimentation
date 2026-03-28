# Git Hygiene

This guide covers what belongs in git, what doesn't, and how to configure `.gitignore` and `.gitattributes` for the Kaizen multi-agent workflow.

## Principle

Track **source code and shared configuration**. Ignore **build artifacts, agent runtime state, and user-local overrides**. If a file is deterministically regenerated from something already tracked, it probably shouldn't be tracked.

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
- `sql/migrations/` — PostgreSQL DDL (append-only, never modify existing migrations)
- `delta/` — Delta Lake table schemas
- `test-vectors/` — Hash parity test vectors

### Agent Configuration (shared, committed)
- `.claude/settings.json` — Project-level Claude Code settings (Agent Teams flag, pre-approved permissions)
- `.claude/agents/*.md` — Project-scoped subagent definitions (e.g., pr-triage)
- `.multiclaude/config.json` — Multiclaude repo-level settings
- `.multiclaude/agents/*.md` — The 7 Kaizen agent definitions

### Documentation
- `CLAUDE.md` — Agent context (read by all sessions on spawn)
- `AGENTS.md` — Jules agent context
- `README.md`, `CONTRIBUTING.md`
- `docs/` — All documentation, ADRs, coordination files, guides

### Merge Configuration
- `.gitattributes` — Merge drivers for status files, LFS patterns
- `.gitignore` — Exclusion rules

## Files That Must NOT Be Tracked

### Build Artifacts
- `target/` — Rust build output
- `node_modules/` — npm packages
- `.next/` — Next.js build cache
- `dist/` — TypeScript build output
- `ui/tsconfig.tsbuildinfo` — TypeScript incremental compilation cache
- `*.dylib`, `*.so`, `*.dll` — Compiled libraries
- `*.wasm` (generated) — WASM output from `wasm-pack`

### Agent Runtime State
- `.Jules/` — Jules session state (including `palette.md`)
- `.claude/settings.local.json` — User-specific Claude Code overrides
- `.claude/worktrees/` — Claude Code worktree sessions
- `.claude/teams/` — Agent Teams session state
- `.claude/tasks/` — Agent Teams task queues
- `.claude/credentials/` — Auth tokens
- `.claude/statsig/` — Feature flag evaluation cache
- `.claude/cache/` — General cache
- `.claude/*.log` — Session logs
- `.multiclaude/state/` — Daemon state (worker registry, health)
- `.multiclaude/messages/` — Inter-agent mailbox
- `.multiclaude/locks/` — File locks for task claiming
- `.multiclaude/worktrees/` — Per-worker git worktrees
- `.multiclaude/*.pid` — Daemon PID files
- `.multiclaude/*.log` — Daemon and worker logs

### Environment and Secrets
- `.env`, `.env.local`, `.env.*.local` — Environment variables
- `*.pem`, `*.key` — Certificates and keys

## .gitignore

Append these rules to your `.gitignore`:

```gitignore
# === Build artifacts ===
target/
node_modules/
.next/
dist/
*.dylib
*.so
*.dll
ui/tsconfig.tsbuildinfo

# === Agent runtime state ===

# Jules
.Jules/

# Claude Code
.claude/settings.local.json
.claude/worktrees/
.claude/teams/
.claude/tasks/
.claude/credentials/
.claude/statsig/
.claude/cache/
.claude/*.log

# Multiclaude
.multiclaude/state/
.multiclaude/messages/
.multiclaude/locks/
.multiclaude/worktrees/
.multiclaude/*.pid
.multiclaude/*.log

# === Environment and secrets ===
.env
.env.local
.env.*.local
*.pem
*.key
```

## .gitattributes

```gitattributes
# Status files: always accept incoming version on merge conflicts
# (status files are single-writer, append-only — newer is always correct)
docs/coordination/status/** merge=theirs-status

# Lock files: mark as generated (show in diff but don't merge manually)
Cargo.lock linguist-generated=true
go.sum linguist-generated=true
go.work.sum linguist-generated=true
package-lock.json linguist-generated=true

# Proto generated code: mark as generated
crates/experimentation-proto/src/**/*.rs linguist-generated=true
```

Configure the status file merge driver (run once):
```bash
git config merge.theirs-status.name "Always accept incoming status file"
git config merge.theirs-status.driver "cp %B %A"
```

## Recovering from Accidental Tracking

If a build artifact was accidentally committed:

```bash
# Remove from git tracking (keeps local file)
git rm --cached <file>

# Verify it's in .gitignore
grep -q "<pattern>" .gitignore || echo "<pattern>" >> .gitignore

# Commit the removal
git commit -m "chore: stop tracking <file> (build artifact)"

# If multiple open branches have the file, rebase them all
# See docs/guides/merge-conflict-resolution.md for the batch rebase script
```

## Verifying Hygiene

Run periodically to check for accidentally tracked files:

```bash
# Check for common build artifacts in git
git ls-files | grep -E '\.tsbuildinfo|\.next/|node_modules/|target/|dist/|\.Jules/'

# Check for large files that shouldn't be tracked
git ls-files | xargs ls -la 2>/dev/null | awk '$5 > 1000000 {print $5, $9}' | sort -rn

# Check for secrets patterns
git ls-files | xargs grep -l "PRIVATE KEY\|API_KEY\|SECRET" 2>/dev/null
```

If any of these return results, investigate and fix.

## Why These Choices

**`go.work.sum` is tracked** because it's the workspace-level equivalent of `go.sum` — it contains cryptographic checksums for reproducible builds. Without it, `go` commands re-download and re-verify modules.

**`tsconfig.tsbuildinfo` is NOT tracked** because it contains local filesystem timestamps and hashes. It's different for every developer and every CI run. TypeScript regenerates it automatically.

**`.claude/settings.json` is tracked but `.claude/settings.local.json` is NOT** because the project-level config (Agent Teams flag, pre-approved commands) applies to all contributors, while user-level overrides (model preference, API key path) are personal.

**`.multiclaude/agents/*.md` are tracked** because agent definitions are functionally equivalent to CI workflow definitions — they specify how automated workers behave. Changes should go through code review.

**`.Jules/` is NOT tracked** because Jules rebuilds its session state on each task. The `AGENTS.md` file at the repo root provides Jules with project context instead.
