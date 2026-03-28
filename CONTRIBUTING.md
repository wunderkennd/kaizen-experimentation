# Contributing to the Experimentation Platform

This guide covers the conventions and workflows that keep our multi-agent
development model coherent. Every agent — and every human — should read this
before their first PR.

## Quick Start

```bash
git clone <repo-url> && cd experimentation-platform
cp .env.example .env
just setup    # Starts infra, generates code, installs deps, seeds data, runs tests
```

## Branch Naming

All branches follow the pattern:

```
<agent>/<type>/<short-description>
```

Examples:

```
agent-1/feat/adr-016-get-slate-assignment
agent-4/feat/adr-015-avlm
agent-7/port/m7-rust-crud
agent-5/fix/adr-020-adaptive-n-zone-boundary
agent-6/chore/upgrade-recharts
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`, `port`

**Never use auto-generated worker names** (e.g., `worker-swift-eagle`) as branch
names. Always name branches by the feature or ADR being implemented. This makes
PR triage, crash recovery, and `git log` readable.

The `main` branch is protected. All changes land via pull request.

## Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

Scope is the module number or crate name:

```
feat(m1): add GetSlateAssignment RPC for ADR-016
fix(experimentation-stats): handle zero-variance arms in CUPED
test(m5): add integration tests for adaptive N zone classification
docs(adr-011): document decision on multi-objective reward composition
port(m7): migrate flag CRUD from Go to Rust
```

Breaking changes use a `!` suffix: `feat(m1)!: change bucket() return type to u32`

## Pull Request Process

1. **Create your branch** from `main` using the naming convention above.
2. **Keep PRs focused.** One logical change per PR. If a feature touches
   multiple modules, coordinate via the contract versioning protocol below.
3. **Update your status file** in the same PR or a follow-up commit:
   `docs/coordination/status/agent-N-status.md`.
4. **Fill in the PR template** (auto-populated when you open a PR).
5. **All CI checks must pass** before merge: lint, test, hash parity, type check,
   `buf breaking`.
6. **One approving review** required. Cross-module PRs require review from the
   affected module's agent.
7. **Squash merge** to `main`. The squash commit message should follow
   conventional commit format.

### PR Template

```markdown
## Summary
<!-- What does this PR do? Which module(s) and ADR(s) does it affect? -->

## Agent
<!-- e.g. Agent-4 (Statistical Analysis) -->

## Type
<!-- feat | fix | refactor | test | docs | chore | perf | port -->

## Related ADRs
<!-- e.g. ADR-015, ADR-018 -->

## Contract Changes
<!-- If this PR changes a proto schema, API contract, or shared crate interface:
     - Which contract version is bumped?
     - Have you notified downstream agents?
     - Is there a migration path? -->

## Testing
<!-- How was this tested? Include relevant commands. -->
- [ ] Unit tests pass (`cargo test -p <crate>` / `go test ./...` / `npm test`)
- [ ] Integration tests pass (if applicable)
- [ ] Golden-file validation (if new statistical method)
- [ ] Proptest invariants added (if new public function in experimentation-stats)
- [ ] Hash parity validated (if touching hash/assignment)
- [ ] `buf lint` + `buf breaking` pass (if touching proto/)

## Status File
- [ ] `docs/coordination/status/agent-N-status.md` updated

## Checklist
- [ ] Code follows project conventions
- [ ] Proto changes are backward-compatible (or ADR documents the break)
- [ ] Documentation updated (if user-facing)
- [ ] No new warnings from linters
```

## Contract Versioning Protocol

When an agent changes a shared interface (proto schema, Rust crate public API,
Go package exported types), the following protocol applies:

### Proto Schema Changes

1. **Additive changes** (new fields, new RPCs, new messages): No version bump
   needed. Add fields with new tag numbers. Downstream agents pick up changes
   on their next `just codegen`.

2. **Breaking changes** (removed fields, renamed messages, changed semantics):
   Requires a new ADR documenting the rationale. The agent proposing the break
   must open a coordination PR that:
   - Updates the proto file
   - Updates all call sites they own
   - Lists affected downstream agents in the PR description
   - Tags downstream agents for review

3. **`buf breaking`** runs in CI against the `main` branch. Breaking changes
   that haven't gone through the protocol will fail CI.

### Rust Crate API Changes

Shared crates (`experimentation-core`, `experimentation-hash`, `experimentation-proto`)
follow the same additive/breaking distinction:

- **Additive**: New public functions, new enum variants (if `#[non_exhaustive]`),
  new optional fields. No coordination required.
- **Breaking**: Removed or renamed public items, changed signatures, changed
  behavior. Requires cross-agent PR review.

The workspace `Cargo.toml` pins all internal crate versions. Bumps to shared
crates should be called out in the PR description.

## Status File Protocol

Each agent maintains `docs/coordination/status/agent-N-status.md`:

- **Write only your own.** Never edit another agent's status file.
- **Read others as needed.** Check dependencies before starting blocked work.
- **Update on every PR.** The status file should reflect current progress.
- **Merge conflicts**: Always take the incoming version. Status files are
  single-writer and append-only — the newer write is always more accurate.
  See `docs/guides/merge-conflict-resolution.md` for automation via `.gitattributes`.

## Merge Conflict Resolution

### Status files (`docs/coordination/status/`)

Always accept the incoming branch's version:
```bash
git checkout --theirs docs/coordination/status/*.md
git add docs/coordination/status/*.md
git rebase --continue
```

Automate permanently via `.gitattributes`:
```
docs/coordination/status/** merge=theirs-status
```

### Generated / build artifact files

These should not be in git at all. If you encounter a conflict in any of these,
remove them from tracking:
```bash
git rm --cached <file>
# Ensure it's in .gitignore
git rebase --continue
```

Common offenders: `tsconfig.tsbuildinfo`, `.Jules/palette.md`, `node_modules/`,
`.next/`, `target/`.

### Multi-branch rebase after cleanup

When a file is removed from tracking on `main` and multiple open branches
still have it, rebase all branches in a batch:
```bash
for branch in $(gh pr list --json headRefName --jq '.[].headRefName'); do
  git checkout "$branch"
  git rebase main || {
    git rm --cached <problematic-file> 2>/dev/null
    git rebase --continue
  }
  git push --force-with-lease origin "$branch"
done
git checkout main
```

See `docs/guides/merge-conflict-resolution.md` for full details.

## Code Style

### Rust

- Follow `rustfmt` defaults (enforced by CI)
- `clippy --all-features -- -D warnings` must pass
- Use `experimentation_core::Error` for all error types
- Use `assert_finite!()` for any floating-point computation
- Prefer `thiserror` for library crates, `anyhow` only in binary crates

### Go

- Follow `gofmt` and `go vet` (enforced by CI)
- Use `connectrpc.com/connect` for all RPC handlers
- Structured logging via `slog` (JSON format in production, text in dev)
- Context propagation: always pass `context.Context` as first parameter

### TypeScript

- ESLint + Prettier (enforced by CI)
- Strict TypeScript (`"strict": true` in tsconfig)
- Use `@connectrpc/connect-web` for API calls
- Components in `src/components/`, pages in `src/app/`

## Testing Conventions

### Unit Tests

- Rust: `#[cfg(test)] mod tests` in each module, plus `tests/` for integration
- Go: `_test.go` files alongside source, `testify/assert` for assertions
- TypeScript: `vitest` with files in `src/__tests__/`

### Golden Files

Statistical tests use golden files in `crates/experimentation-stats/tests/golden/`.
To update golden files after an intentional algorithm change:

```bash
UPDATE_GOLDEN=1 cargo test --workspace
```

### Property-Based Tests

Rust crates use `proptest` for invariant testing. Nightly CI runs extended
proptest campaigns (10,000 cases, 30-minute timeout).

### Integration Tests

Go integration tests use the `//go:build integration` build tag and run against
`docker-compose.test.yml`:

```bash
just test-integration
```

## Git Hygiene

### Files That Must Be Tracked

- `go.mod`, `go.sum`, `go.work`, `go.work.sum` — reproducible Go builds
- `Cargo.toml`, `Cargo.lock` — reproducible Rust builds
- `package.json`, `package-lock.json` — reproducible Node builds
- `.claude/settings.json` — project-level Claude Code settings
- `.multiclaude/agents/*.md`, `.multiclaude/config.json` — agent definitions
- `.gitattributes` — merge drivers for status files

### Files That Must NOT Be Tracked

- `ui/tsconfig.tsbuildinfo` — TypeScript build cache
- `.Jules/` — Jules agent session state
- `.claude/settings.local.json`, `.claude/worktrees/`, `.claude/teams/`, `.claude/tasks/`
- `.multiclaude/state/`, `.multiclaude/messages/`, `.multiclaude/worktrees/`, `*.pid`, `*.log`
- `node_modules/`, `.next/`, `target/`, `dist/`

See `docs/guides/git-hygiene.md` for the complete rationale and `.gitignore` rules.

## Adding a New ADR

1. Copy the template: `cp docs/adrs/TEMPLATE.md docs/adrs/NNN-short-title.md`
2. Fill in Status, Context, Decision, Consequences
3. Open a PR with the ADR for team review
4. Once approved, update the ADR status to "Accepted"
5. Update `docs/adrs/README.md` index with the new entry
6. If the ADR assigns work to agents, update the relevant `.multiclaude/agents/` definitions
