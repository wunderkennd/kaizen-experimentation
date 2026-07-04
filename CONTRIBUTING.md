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
| `cluster-a` through `cluster-g` | Capability cluster (cluster-g = ADR-029 Personalization Orchestration) |
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

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`, `port`, `design`

(`design` is a repo-specific verb for RFC / ADR design-lock branches; the
others mirror standard conventional-commit verbs. The full allowlist —
including `infra-N/<verb>/<slug>`, `palette/<slug>`, and `chore/<slug>`
families — lives in [`.github/branch-naming.yml`](./.github/branch-naming.yml)
and is enforced by `just check-branch-name` plus the advisory CI check at
[`.github/workflows/branch-naming.yml`](./.github/workflows/branch-naming.yml).)

**Prefer naming branches by the feature or ADR** (`agent-N/<type>/<slug>`) when
you control the name. Harness-generated names that can't be renamed after launch
(Claude Code web/remote sessions → `claude/<slug>`, multiclaude workers →
`work/<slug>`) are *tolerated* — recognized by the allowlist, advisory only —
because agent ownership now rides on **PR metadata** (a Conventional-Commit PR
title plus the `agent-N` label inherited from the linked issue), not the branch
name. See the `pr-title` and `pr-label-inheritance` workflows in
[`.github/workflows/`](./.github/workflows/).

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

The lifecycle is **draft while working → ready = "work complete" → address
reviewer feedback → merge**, and the review-feedback step is enforced by the
`Review gate` check, not left to memory.

1. **Create your branch** from `main` using the naming convention above.
2. **Open the PR as a draft** while work is in progress. Keep PRs focused —
   one logical change per PR — and include `Closes #<number>` so the Issue
   auto-closes on merge.
3. **Fill in the PR template** (auto-populated when you open a PR).
4. **Mark the PR ready for review the moment the work is complete.** Ready
   means "I claim this is done": tests green, merge-ready state. Harness
   workers do this as their final step (or add the `ready` label —
   `auto-ready.yml` flips the draft). Automated review runs on this
   transition.
5. **Address every piece of reviewer feedback before merge — whoever the
   reviewer is** (Devin, Claude review, or a human). For each review thread,
   push the fix (or reply with why not), then click **Resolve conversation**.
   A standing changes-requested review blocks until re-reviewed or dismissed
   with a written rationale. Enforced by `.github/workflows/review-gate.yml`
   (red while any thread is unresolved or changes-requested stands; the
   failure log lists exactly what to address) and natively by
   `required_conversation_resolution` in `.github/settings.yml`.
6. **All required checks must pass**: `PR title check`, `Review gate`,
   `schema`, `rust`, `go`, `typescript`, `hash-parity` (path-skipped jobs
   satisfy the requirement on unrelated PRs). Set in `.github/settings.yml`.
7. **Review is graduated** (owner decision, 2026-07-04 — #681). Routine green
   PRs merge automatically: `automerge.yml` arms auto-merge when the PR is
   ready and carries no risk signal, and the platform merges once the required
   checks (including the Review gate) clear — no blanket human approval.
   **Human review is required** for `breaking`, `contract-test`,
   `needs-human-input`, and proto-touching PRs — auto-merge refuses these and
   requests a reviewer; cross-module PRs should get review from the affected
   module's agent.
8. **Squash merge** to `main` (auto-merge squashes; history stays linear).

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
