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

## Work Tracking

All work is tracked via **GitHub Milestones and Issues**. No in-repo status
files or task trackers.

```
Milestone    =  Sprint (e.g., "Sprint 5.0: Schema & Foundations")
  └── Issue  =  ADR implementation unit (e.g., "ADR-015: AVLM Implementation")
```

### Finding Your Work

```bash
# What's assigned to me?
gh issue list --assignee @me --state open

# What's in the current sprint?
gh issue list --milestone "Sprint 5.0: Schema & Foundations"

# What's blocked?
gh issue list --label "blocked"

# Read a task spec
gh issue view <number>
```

### Updating Progress

- **Comment** on the issue with progress updates (what's done, what's next, blockers)
- **Link PRs** to issues: include `Closes #<number>` in the PR description — the issue auto-closes on merge
- **Label blockers**: add the `blocked` label and comment explaining what you're waiting on and which issue blocks you
- **Never leave an issue in limbo**: if you can't finish, comment with current state so another agent can pick it up

### Labels

| Label | Meaning |
| --- | --- |
| `agent-1` through `agent-7` | Agent ownership |
| `P0` through `P4` | Priority tier |
| `cluster-a` through `cluster-f` | Capability cluster |
| `blocked` | Waiting on another issue or agent |
| `contract-test` | Cross-module contract test |

## Branch Naming

All branches follow the pattern:

```
<agent>/<type>/<short-description>
```

Examples:

```
agent-4/feat/adr-015-avlm
agent-7/port/m7-rust-crud
agent-1/feat/adr-016-get-slate-assignment
agent-5/fix/adr-020-adaptive-n-zone-boundary
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`, `port`

**Never use auto-generated worker names** (e.g., `worker-swift-eagle`) as branch
names. Always name branches by the feature or ADR being implemented.

The `main` branch is protected. All changes land via pull request.

## Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

Closes #<issue-number>
```

Scope is the module number or crate name:

```
feat(experimentation-stats): implement AVLM confidence sequences

Closes #42
```

Breaking changes use a `!` suffix: `feat(m1)!: change bucket() return type to u32`

## Pull Request Process

1. **Create your branch** from `main` using the naming convention above.
2. **Keep PRs focused.** One logical change per PR.
3. **Reference the Issue**: include `Closes #<number>` in the PR description.
   The issue auto-closes when the PR merges.
4. **Fill in the PR template** (auto-populated when you open a PR).
5. **All CI checks must pass** before merge.
6. **One approving review** required. Cross-module PRs require review from the
   affected module's agent.
7. **Squash merge** to `main`.

### PR Template

```markdown
## Summary
<!-- What does this PR do? Which module(s) and ADR(s) does it affect? -->

## Closes
<!-- e.g. Closes #42 -->

## Agent
<!-- e.g. Agent-4 (Statistical Analysis) -->

## Type
<!-- feat | fix | refactor | test | docs | chore | perf | port -->

## Contract Changes
<!-- If this PR changes a proto schema, API contract, or shared crate interface:
     - Which contract version is bumped?
     - Have you notified downstream agents?
     - Is there a migration path? -->

## Testing
- [ ] Unit tests pass (`cargo test -p <crate>` / `go test ./...` / `npm test`)
- [ ] Golden-file validation (if new statistical method)
- [ ] Proptest invariants added (if new public function in experimentation-stats)
- [ ] Hash parity validated (if touching hash/assignment)
- [ ] `buf lint` + `buf breaking` pass (if touching proto/)

## Checklist
- [ ] Branch named `agent-N/<type>/adr-XXX-description`
- [ ] Code follows project conventions
- [ ] PR references the GitHub Issue (`Closes #N`)
- [ ] Proto changes are backward-compatible (or ADR documents the break)
- [ ] Documentation updated (if user-facing)
```

## Contract Versioning Protocol

When an agent changes a shared interface (proto schema, Rust crate public API,
Go package exported types), the following protocol applies:

### Proto Schema Changes

1. **Additive changes** (new fields, new RPCs, new messages): No version bump
   needed. Add fields with new tag numbers.

2. **Breaking changes** (removed fields, renamed messages, changed semantics):
   Requires a new ADR. The proposing agent must open a coordination PR that
   updates the proto, all owned call sites, and tags downstream agents.

3. **`buf breaking`** runs in CI against `main`. Breaking changes that haven't
   gone through the protocol fail CI.

### Rust Crate API Changes

Shared crates (`experimentation-core`, `experimentation-hash`, `experimentation-proto`):

- **Additive**: New public functions, new `#[non_exhaustive]` enum variants.
  No coordination required.
- **Breaking**: Removed or renamed public items, changed signatures. Requires
  cross-agent PR review.

## Code Style

### Rust
- `rustfmt` defaults (CI-enforced)
- `clippy --all-features -- -D warnings` must pass
- `assert_finite!()` for all floating-point computation
- `thiserror` for library crates, `anyhow` only in binary crates

### Go
- `gofmt` and `go vet` (CI-enforced)
- `connectrpc.com/connect` for RPC handlers
- `slog` for structured logging
- Context propagation: `context.Context` as first parameter

### TypeScript
- ESLint + Prettier (CI-enforced)
- Strict TypeScript
- `@connectrpc/connect-web` for API calls

## Testing Conventions

### Golden Files
Located at `crates/experimentation-stats/tests/golden/`. Update after intentional
algorithm changes: `UPDATE_GOLDEN=1 cargo test --workspace`

### Property-Based Tests
`proptest` for invariant testing. Nightly CI: 10,000 cases, 30-minute timeout.

### Integration Tests
Go: `//go:build integration` tag, run with `just test-integration`

## Git Hygiene

### Must Be Tracked
`go.work.sum`, `Cargo.lock`, `package-lock.json`, `.claude/settings.json`,
`.multiclaude/agents/*.md`, `.multiclaude/config.json`, `.gitattributes`

### Must NOT Be Tracked
`tsconfig.tsbuildinfo`, `.Jules/`, `.claude/settings.local.json`,
`.claude/worktrees/`, `.multiclaude/state/`, `node_modules/`, `target/`, `.next/`

See `docs/guides/git-hygiene.md` for complete rules.

## Merge Conflict Resolution

| File Type | Strategy |
| --- | --- |
| `Cargo.lock` | Accept either version, run `cargo generate-lockfile` |
| `go.sum` / `go.work.sum` | Accept either version, run `go mod tidy` |
| Proto schema | Never auto-resolve — check tag numbers, review manually |
| Build artifacts | Remove from git entirely (should be in `.gitignore`) |
| Source code | Standard merge resolution |

See `docs/guides/merge-conflict-resolution.md` for detailed procedures.

## Adding a New ADR

1. Copy the template: `cp docs/adrs/TEMPLATE.md docs/adrs/NNN-short-title.md`
2. Fill in Status, Context, Decision, Consequences
3. Open a PR with the ADR for review
4. Once approved, update status to "Accepted" and update `docs/adrs/README.md`
5. Create GitHub Issues for the implementation work
6. Update relevant `.multiclaude/agents/` definitions if the ADR assigns work
